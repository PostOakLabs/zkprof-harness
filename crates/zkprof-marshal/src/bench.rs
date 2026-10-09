// SPDX-License-Identifier: Apache-2.0

//! The measurement, split across the boundary.
//!
//! # Why the loop lives in the host
//!
//! A Rust loop cannot measure the JS↔WASM boundary, because a Rust loop does
//! not cross it. The crossing is the thing under test, so the host owns the
//! loop and Rust owns the work: the host calls in once per iteration, and this
//! module times what happens on its side of each call.
//!
//! That inverts the usual shape of a [`Profiler`] run, and it is the reason
//! [`Profiler::set_region_ms`] and [`Profiler::record`] exist. The profiler's
//! own clock sees only the inside-wasm part of every iteration; the host's
//! figure is the only one that covers the whole region.
//!
//! # The protocol
//!
//! 1. [`begin`] — choose a [`Shape`], open a profiler.
//! 2. The host calls [`call`] some small number of times and times it.
//! 3. [`size`] — turn that probe into an iteration count, discard the probe's
//!    own measurements, and open the real measured region.
//! 4. The host calls [`call`] that many times, timing the whole loop.
//! 5. [`report`] — hand the host's total back and get a report.
//!
//! # How the loop is attributed
//!
//! Every shape spans its own work in one primitive: [`Primitive::Hash`] for
//! the Poseidon compressions, [`Primitive::Marshalling`] for the copy
//! baseline's byte traversal. Everything else in the loop — the
//! wasm-bindgen glue's copy in and out, byte↔field conversion, the JavaScript
//! call itself, and the host's two clock reads per iteration — is the
//! remainder, and lands in `marshalling`:
//!
//! `marshalling = host_total − shape_work`
//!
//! For the copy baseline the shape work *is* `marshalling`, so the two merge
//! and the row carries the entire loop, which is what a baseline should do.
//!
//! # What biases this
//!
//! The Poseidon shapes take two clock readings per call to time their work. At
//! a 5 µs browser quantum that is well under a percent of a one-hash call, and
//! it shrinks as the batch grows. The copy baseline takes **none**: it spans
//! nothing, because a clock read inside wasm would inflate the very boundary
//! it exists to measure. That asymmetry is deliberate, and it is why the two
//! shapes are not instrumented the same way.

use std::cell::{Cell, RefCell};

use ark_bn254::Fr;
use ark_crypto_primitives::sponge::poseidon::PoseidonConfig;
use ark_ff::{BigInteger, PrimeField};
use zkprof_backend_arkworks::circuit::{compress, poseidon_config};
use zkprof_core::clock::probe_resolution;
use zkprof_core::model::{Memory, Primitive, Report, System, Target};
use zkprof_core::profiler::Profiler;
use zkprof_core::scale::{scale_iterations, Scaling};
use zkprof_core::{linear_memory_bytes, platform_clock, Clock, DEFAULT_PROBE_SAMPLES};

use crate::shape::{Shape, COPY_SUM_BYTES, FIELD_BYTES};
use crate::Error;

thread_local! {
    /// The clock, leaked once per thread.
    ///
    /// [`Profiler<'c>`] borrows its clock for as long as it lives, and a bench
    /// outlives every function frame in this module, so the clock has to have
    /// a lifetime longer than any borrow the profiler could take. Leaking one
    /// `Box<dyn Clock>` per thread is the smallest thing that satisfies that,
    /// and it is bounded: one allocation per thread, for the thread's life.
    static CLOCK: Cell<Option<&'static dyn Clock>> = const { Cell::new(None) };

    /// The bench in progress, if any. One per thread, which on wasm32 means
    /// one per module instance.
    static RUN: RefCell<Option<Bench>> = const { RefCell::new(None) };
}

/// The clock for this thread, constructing and leaking it on first use.
///
/// # Errors
///
/// [`zkprof_core::ClockError`] on `wasm32` outside a JavaScript realm. Never
/// fails natively.
fn clock() -> Result<&'static dyn Clock, Error> {
    CLOCK.with(|slot| {
        if let Some(clock) = slot.get() {
            return Ok(clock);
        }
        let leaked: &'static dyn Clock = Box::leak(platform_clock()?);
        slot.set(Some(leaked));
        Ok(leaked)
    })
}

/// A bench in progress: the shape, the profiler accumulating it, and the
/// Poseidon parameters the shape's work needs.
struct Bench {
    profiler: Profiler<'static>,
    shape: Shape,
    params: PoseidonConfig<Fr>,
}

impl Bench {
    /// Replaces the profiler with a fresh one for the same shape.
    ///
    /// Used by [`size`] to drop the probe batch: those calls sized the run and
    /// are not part of it, and leaving their spans in the accumulator would
    /// mix a handful of cold-cache iterations into the measurement.
    fn reopen(&mut self) -> Result<(), Error> {
        self.profiler = open_profiler(self.shape)?;
        Ok(())
    }
}

/// Opens a profiler for `shape`.
fn open_profiler(shape: Shape) -> Result<Profiler<'static>, Error> {
    Ok(Profiler::new(
        clock()?,
        target(),
        system(shape),
        shape.workload_id(),
    ))
}

/// Starts a bench of `shape`, replacing any bench already in progress.
///
/// # Errors
///
/// - [`Error::Shape`] if the shape's payload is empty or over the ceiling.
/// - [`Error::Clock`] if no clock could be constructed.
pub fn begin(shape: Shape) -> Result<(), Error> {
    shape.validate()?;
    // Built before the profiler opens so the Grain LFSR's cost is outside
    // every region, measured or not.
    let params = poseidon_config::<Fr>();
    let profiler = open_profiler(shape)?;
    RUN.with(|slot| {
        *slot.borrow_mut() = Some(Bench {
            profiler,
            shape,
            params,
        });
    });
    Ok(())
}

/// One boundary crossing: does this shape's work for one payload.
///
/// The caller must pass exactly [`Shape::payload_len`] bytes; the return is
/// [`Shape::result_len`] bytes. The payload is returned rather than discarded
/// so the work cannot be optimised away, and so the outbound copy — a real
/// part of the boundary's cost — is included in what the host measures.
///
/// # Errors
///
/// - [`Error::NotBegun`] if no bench is in progress.
/// - [`Error::WrongPayloadLen`] if `input` is not this shape's payload size.
pub fn call(input: &[u8]) -> Result<Vec<u8>, Error> {
    let expected = current_shape().ok_or(Error::NotBegun)?.payload_len();
    if input.len() != expected {
        return Err(Error::WrongPayloadLen {
            expected,
            actual: input.len(),
        });
    }

    RUN.with(|slot| {
        let mut slot = slot.borrow_mut();
        let bench = slot.as_mut().ok_or(Error::NotBegun)?;

        match bench.shape {
            Shape::Poseidon { .. } => {
                let params = &bench.params;
                // Decode, hash, encode as three passes rather than one fused
                // loop, so the span covers the compressions and nothing else.
                // The two conversion passes are serialisation, and
                // serialisation belongs to `marshalling`, which is where the
                // remainder puts it.
                // The payload length was checked against the shape above, so
                // the remainders here are empty by construction.
                let (elements, _remainder) = input.as_chunks::<FIELD_BYTES>();
                let fields: Vec<Fr> = elements
                    .iter()
                    .map(|chunk| Fr::from_le_bytes_mod_order(chunk))
                    .collect();
                let (pairs, _remainder) = fields.as_chunks::<2>();
                let digests: Vec<Fr> = bench.profiler.span(Primitive::Hash, || {
                    pairs
                        .iter()
                        .map(|pair| compress(params, pair[0], pair[1]))
                        .collect()
                });
                Ok(encode_fields(&digests))
            }
            Shape::Copy { .. } => {
                // Unspanned on purpose. See the module docs: any clock read
                // here would inflate the boundary this shape measures.
                Ok(copy_sum(input).to_vec())
            }
        }
    })
}

/// Turns a probe batch into an iteration count and opens the measured region.
///
/// `calls` is how many times the host called [`call`] while timing
/// `elapsed_ms`. The answer is [`zkprof_core::scale`]'s: the count that puts
/// the loop at least 1000× above the measured clock resolution, capped, with
/// `target_met: false` when the cap is what stopped it.
///
/// The probe batch's own spans are discarded.
///
/// # Errors
///
/// [`Error::NotBegun`] if no bench is in progress.
pub fn size(calls: u32, elapsed_ms: f64) -> Result<Scaling, Error> {
    RUN.with(|slot| {
        let mut slot = slot.borrow_mut();
        let bench = slot.as_mut().ok_or(Error::NotBegun)?;

        let scaling = scale_iterations(
            elapsed_ms * 1_000.0,
            calls,
            bench.profiler.resolution().lower_bound_us(),
        );
        bench.reopen()?;
        Ok(scaling)
    })
}

/// Closes the measured region and builds the report.
///
/// `calls` is how many times the host called [`call`] while timing
/// `elapsed_ms`, which becomes the report's `iterations` and `total_ms`. The
/// bench is consumed; a further [`report`] without a new [`begin`] is an
/// [`Error::NotBegun`].
///
/// # Errors
///
/// - [`Error::NotBegun`] if no bench is in progress.
/// - [`Error::Report`] if the assembled report fails validation, which
///   includes the case where nothing measurable was recorded.
pub fn report(calls: u32, elapsed_ms: f64) -> Result<Report, Error> {
    let bench = RUN.with(|slot| slot.borrow_mut().take().ok_or(Error::NotBegun))?;
    let shape = bench.shape;
    let total_us = elapsed_ms * 1_000.0;
    let work_us = bench.profiler.primitive_us(shape.primitive());

    let profiler = bench.profiler.with_iterations(calls.max(1));
    profiler.set_region_ms(elapsed_ms);
    // The remainder of the loop. For the copy baseline `work_us` is zero, so
    // this is the whole loop; for Poseidon it is everything the hash span did
    // not cover.
    profiler.record(Primitive::Marshalling, total_us - work_us);

    // A high-water mark is only visible at the end, and it describes the
    // module's linear memory rather than this workload's allocations: wasm32
    // grows linear memory and does not hand it back. Across a suite run in
    // one module instance the figure is therefore a running maximum, which is
    // the same caveat the prover harness carries.
    if let Some(bytes) = linear_memory_bytes() {
        profiler.record_memory(Memory::peak_heap_bytes(bytes));
    }

    Ok(profiler.finish()?)
}

/// The shape of the bench in progress, if any.
///
/// Lets the wasm surface answer "how big is this workload's payload" without
/// making the host restate the shape it just chose.
#[must_use]
pub fn current_shape() -> Option<Shape> {
    RUN.with(|slot| slot.borrow().as_ref().map(|bench| bench.shape))
}

/// The measured resolution of the clock this module times with, µs.
///
/// A direct probe rather than a property of any particular bench, so the host
/// can see the clock it is about to be measured by before it starts, and
/// without disturbing a bench already in progress.
///
/// # Errors
///
/// [`Error::Clock`] if no clock could be constructed.
pub fn resolution_us() -> Result<f64, Error> {
    Ok(probe_resolution(clock()?, DEFAULT_PROBE_SAMPLES).lower_bound_us())
}

/// Throws away the bench in progress, if any.
///
/// Lets the host abandon a run it cannot finish — a worker being torn down, a
/// shape the page decided not to measure — without leaving a profiler open
/// behind it.
pub fn abort() {
    RUN.with(|slot| *slot.borrow_mut() = None);
}

/// Serialises field elements the way [`Shape::Poseidon`] returns them.
fn encode_fields(fields: &[Fr]) -> Vec<u8> {
    let mut out = Vec::with_capacity(fields.len() * FIELD_BYTES);
    for field in fields {
        out.extend_from_slice(&field.into_bigint().to_bytes_le());
    }
    out
}

/// The copy baseline's payload work: a byte sum, returned little-endian.
///
/// Sums rather than echoes so the bytes must actually be read — an echo could
/// in principle be satisfied without traversing the input — while the return
/// stays a fixed eight bytes. That makes this a floor on the *inbound* half of
/// the crossing, which is the half the Poseidon shapes are dominated by.
fn copy_sum(input: &[u8]) -> [u8; COPY_SUM_BYTES] {
    let mut sum: u64 = 0;
    for &byte in input {
        sum = sum.wrapping_add(u64::from(byte));
    }
    sum.to_le_bytes()
}

/// The target a report describes.
fn target() -> Target {
    #[cfg(target_arch = "wasm32")]
    {
        zkprof_core::browser::browser_target()
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        Target::native_with_cpu(zkprof_core::browser::cpu_description())
    }
}

/// The system a report describes.
fn system(shape: Shape) -> System {
    match shape {
        Shape::Poseidon { .. } => System::new("poseidon", "bn254"),
        // The copy baseline touches no curve and no proof system. The schema
        // requires both strings, so they say so rather than naming a system
        // that is not involved in the measurement.
        Shape::Copy { .. } => System::new("none", "none"),
    }
}

#[cfg(test)]
mod tests {
    use super::{abort, begin, call, copy_sum, encode_fields, report, size, system};
    use crate::shape::Shape;
    use ark_bn254::Fr;
    use ark_ff::PrimeField;
    use zkprof_core::model::Primitive;

    /// A payload of `len` deterministic bytes. The pattern matters only in
    /// that two different lengths give different sums.
    fn payload(len: usize) -> Vec<u8> {
        (0..len)
            .map(|index| u8::try_from(index % 251).unwrap())
            .collect()
    }

    #[test]
    fn the_copy_baseline_sums_the_bytes_it_was_given() {
        // 1 + 2 + 3 = 6, little-endian.
        assert_eq!(copy_sum(&[1, 2, 3]), [6, 0, 0, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn the_copy_baseline_wraps_rather_than_overflowing() {
        // 255 * 8 = 2040, which fits; 255 * 2^32 would not, and `wrapping_add`
        // is what keeps that from panicking in a release build.
        assert_eq!(copy_sum(&[255; 8]), [248, 7, 0, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn a_field_round_trips_through_the_wire_format() {
        let field = Fr::from_le_bytes_mod_order(&[7, 0, 0, 0, 0, 0, 0, 0]);
        let bytes = encode_fields(&[field]);
        assert_eq!(bytes.len(), 32);
        assert_eq!(Fr::from_le_bytes_mod_order(&bytes), field);
    }

    #[test]
    fn one_poseidon_call_returns_one_digest_per_pair() {
        begin(Shape::Poseidon { batch: 4 }).expect("poseidon shape");
        let out = call(&payload(4 * 64)).expect("matching payload");
        assert_eq!(out.len(), 4 * 32);
        abort();
    }

    #[test]
    fn a_copy_call_returns_eight_bytes() {
        begin(Shape::Copy { bytes: 512 }).expect("copy shape");
        let out = call(&payload(512)).expect("matching payload");
        assert_eq!(out.len(), 8);
        abort();
    }

    #[test]
    fn a_wrong_sized_payload_is_refused_with_both_lengths() {
        begin(Shape::Copy { bytes: 64 }).expect("copy shape");
        let error = call(&payload(32)).expect_err("32 is not 64");
        assert!(
            error.to_string().contains("64") && error.to_string().contains("32"),
            "the message should name both lengths: {error}"
        );
        abort();
    }

    #[test]
    fn calling_without_beginning_is_an_error() {
        abort();
        assert!(matches!(call(&[0]), Err(crate::Error::NotBegun)));
        assert!(matches!(size(1, 1.0), Err(crate::Error::NotBegun)));
        assert!(matches!(report(1, 1.0), Err(crate::Error::NotBegun)));
    }

    #[test]
    fn a_report_consumes_the_bench() {
        begin(Shape::Copy { bytes: 64 }).expect("copy shape");
        call(&payload(64)).expect("matching payload");
        report(1, 0.5).expect("a report");
        // Consumed: a second report without a new begin is NotBegun.
        assert!(matches!(report(1, 0.5), Err(crate::Error::NotBegun)));
    }

    #[test]
    fn the_copy_baseline_attributes_the_whole_loop_to_marshalling() {
        begin(Shape::Copy { bytes: 64 }).expect("copy shape");
        for _ in 0..4 {
            call(&payload(64)).expect("matching payload");
        }
        let report = report(4, 20.0).expect("a report");

        assert_eq!(report.primitives.len(), 1);
        assert_eq!(report.primitives[0].name, Primitive::Marshalling);
        assert!((report.primitives[0].share - 1.0).abs() < 1e-9);
        // The whole 20 ms, not just the part Rust saw.
        assert!((report.primitives[0].ms - 20.0).abs() < 1e-9);
        assert!((report.total_ms - 20.0).abs() < 1e-9);
        assert_eq!(report.iterations, 4);
    }

    #[test]
    fn poseidon_splits_the_loop_between_hash_and_marshalling() {
        begin(Shape::Poseidon { batch: 2 }).expect("poseidon shape");
        for _ in 0..3 {
            call(&payload(2 * 64)).expect("matching payload");
        }
        // 12 ms of host time; whatever the compressions cost, the rest is
        // marshalling.
        let report = report(3, 12.0).expect("a report");

        let hash = report.primitive(Primitive::Hash).expect("a hash row");
        let marshalling = report
            .primitive(Primitive::Marshalling)
            .expect("a marshalling row");

        assert!(hash.ms > 0.0, "three batches of Poseidon should register");
        assert!((hash.ms + marshalling.ms - 12.0).abs() < 1e-6);
        assert!((hash.share + marshalling.share - 1.0).abs() < 1e-9);
        assert!((report.total_ms - 12.0).abs() < 1e-9);
    }

    #[test]
    fn the_host_total_is_what_the_report_carries_not_what_rust_saw() {
        // The point of the whole design: a region mostly spent outside wasm
        // must still be reported at its full size.
        begin(Shape::Copy { bytes: 64 }).expect("copy shape");
        call(&payload(64)).expect("matching payload");
        let report = report(1, 5_000.0).expect("a report");
        assert!((report.total_ms - 5_000.0).abs() < 1e-9);
    }

    #[test]
    fn sizing_returns_a_usable_count() {
        begin(Shape::Copy { bytes: 64 }).expect("copy shape");
        for _ in 0..8 {
            call(&payload(64)).expect("matching payload");
        }
        // 8 calls in 1.6 ms == 200 us each.
        let scaling = size(8, 1.6).expect("a plan");
        assert!(scaling.iterations >= 1);
        assert!(scaling.observed, "a 1.6 ms batch cannot be below any clock");
        assert!((scaling.per_op_us - 200.0).abs() < 1e-6);
        abort();
    }

    #[test]
    fn sizing_discards_the_probe_batch() {
        begin(Shape::Poseidon { batch: 1 }).expect("poseidon shape");
        for _ in 0..8 {
            call(&payload(64)).expect("matching payload");
        }
        let _ = size(8, 1.6).expect("a plan");

        // Nothing has been hashed since the probe was discarded, so there is
        // no hash row at all. If the eight probe calls had survived, eight
        // spans' worth of Poseidon would be sitting in it.
        let report = report(1, 1.0).expect("a report");
        assert!(
            report.primitive(Primitive::Hash).is_none(),
            "the probe batch leaked into the measured run: {:?}",
            report.primitives
        );
    }

    #[test]
    fn a_workload_measured_after_sizing_records_only_its_own_calls() {
        begin(Shape::Poseidon { batch: 1 }).expect("poseidon shape");
        for _ in 0..8 {
            call(&payload(64)).expect("matching payload");
        }
        let _ = size(8, 1.6).expect("a plan");

        // One call after the reset, then report it.
        call(&payload(64)).expect("matching payload");
        let one = report(1, 1.0).expect("a report");
        let one_us = one.primitive(Primitive::Hash).expect("a hash row").ms;

        // The same again, but nine calls instead of one.
        begin(Shape::Poseidon { batch: 1 }).expect("poseidon shape");
        call(&payload(64)).expect("matching payload");
        let _ = size(1, 0.2).expect("a plan");
        for _ in 0..9 {
            call(&payload(64)).expect("matching payload");
        }
        let nine = report(9, 9.0).expect("a report");
        let nine_us = nine.primitive(Primitive::Hash).expect("a hash row").ms;

        assert!(one_us > 0.0, "one Poseidon hash should register");
        // Nine calls cost nine times what one does. This is the assertion
        // that would fail if the reset left the probe's spans behind.
        let ratio = nine_us / one_us;
        assert!(
            (4.0..=16.0).contains(&ratio),
            "nine calls cost {nine_us} ms against one call's {one_us} ms (ratio {ratio})"
        );
    }

    #[test]
    fn an_iteration_count_of_zero_is_reported_as_one() {
        begin(Shape::Copy { bytes: 64 }).expect("copy shape");
        call(&payload(64)).expect("matching payload");
        let report = report(0, 1.0).expect("a report");
        assert_eq!(report.iterations, 1);
    }

    #[test]
    fn a_nothing_run_produces_no_report_rather_than_zero_shares() {
        begin(Shape::Copy { bytes: 64 }).expect("copy shape");
        // No calls, no elapsed time: nothing was measured.
        assert!(report(1, 0.0).is_err());
    }

    #[test]
    fn the_system_names_the_curve_only_for_poseidon() {
        assert_eq!(
            system(Shape::Poseidon { batch: 1 }).proof_system,
            "poseidon"
        );
        assert_eq!(system(Shape::Poseidon { batch: 1 }).curve_or_field, "bn254");
        assert_eq!(system(Shape::Copy { bytes: 8 }).proof_system, "none");
        assert_eq!(system(Shape::Copy { bytes: 8 }).curve_or_field, "none");
    }

    #[test]
    fn poseidon_is_deterministic_across_calls() {
        // Same payload in, same digest out. If the params were rebuilt per
        // call, or the state leaked between calls, this would drift.
        begin(Shape::Poseidon { batch: 2 }).expect("poseidon shape");
        let first = call(&payload(128)).expect("matching payload");
        let second = call(&payload(128)).expect("matching payload");
        assert_eq!(first, second);
        abort();
    }
}
