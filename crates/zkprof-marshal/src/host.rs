// SPDX-License-Identifier: Apache-2.0

//! The data the host exchanges with the bench, and the arithmetic derived
//! from a report.
//!
//! # Why this is not in [`crate::bindings`]
//!
//! Everything here is plain Rust: no `wasm-bindgen`, no `web-sys`, no clock,
//! no browser. That is deliberate and it is the whole reason this module
//! exists. [`crate::bindings`] is `wasm32`-only, so anything living there is
//! invisible to `cargo test` on the CI machine — which would leave the
//! per-hash and per-byte arithmetic, the numbers this bench exists to
//! produce, permanently untested while looking covered.
//!
//! So the split is: this module holds everything that can be reasoned about
//! without a browser, and [`crate::bindings`] is reduced to thin
//! `#[wasm_bindgen]` wrappers that own no logic.
//!
//! # Why the derived figures are computed here and not in the page
//!
//! Per-call and per-hash microseconds are arithmetic on `total_ms`,
//! `iterations` and the batch size. Leaving the page to redo that arithmetic
//! is how a page and a CLI end up printing different answers from the same
//! report. [`Derived`] is named for what it is: division, not measurement.

use serde::Serialize;
use zkprof_core::model::Report;
use zkprof_core::scale::Scaling;

use crate::shape::{self, Shape};
use crate::Error;

/// One workload the suite asks the page to run.
#[derive(Debug, Clone, Serialize)]
pub struct WorkloadSpec {
    /// `poseidon` or `copy`.
    pub kind: String,
    /// Batch size for `poseidon`, byte count for `copy`.
    pub param: u32,
    /// The `workload_id` the resulting report carries.
    pub workload_id: String,
    /// Bytes the page must pass to each call.
    pub payload_len: usize,
    /// Bytes each call returns.
    pub result_len: usize,
    /// Short human label for a table row.
    pub label: String,
}

impl WorkloadSpec {
    /// Describes `shape` for the page.
    #[must_use]
    pub fn of(shape: &Shape) -> Self {
        let (kind, param, label) = match *shape {
            Shape::Poseidon { batch } => ("poseidon".to_owned(), batch, format!("batch {batch}")),
            Shape::Copy { bytes } => ("copy".to_owned(), bytes, format!("{bytes} B")),
        };
        Self {
            kind,
            param,
            workload_id: shape.workload_id(),
            payload_len: shape.payload_len(),
            result_len: shape.result_len(),
            label,
        }
    }

    /// The suite, in the order the page should run it.
    ///
    /// Each Poseidon batch is followed by the copy baseline carrying the same
    /// number of bytes, so the two columns of a results table line up.
    #[must_use]
    pub fn suite() -> Vec<Self> {
        shape::suite().iter().map(Self::of).collect()
    }

    /// The shape this spec describes.
    ///
    /// The page drives the bench through `marshal_begin`, which takes the
    /// `(kind, param)` pair separately, so this is the inverse of the split
    /// [`WorkloadSpec::of`] performs. Keeping it next to `of` means the two
    /// halves cannot drift apart silently.
    ///
    /// # Errors
    ///
    /// [`Error::UnknownShape`] if this spec's `kind` is neither `poseidon`
    /// nor `copy`, naming the kind it found.
    pub fn shape(&self) -> Result<Shape, Error> {
        parse_shape(&self.kind, self.param)
    }
}

/// Turns a `(kind, param)` pair from the host into a [`Shape`].
///
/// `kind` is `poseidon` or `copy`; `param` is the batch size or the byte
/// count.
///
/// # Errors
///
/// [`Error::UnknownShape`] for any other `kind`, naming it.
#[must_use = "the shape is the point of the conversion"]
pub fn parse_shape(kind: &str, param: u32) -> Result<Shape, Error> {
    match kind {
        "poseidon" => Ok(Shape::Poseidon { batch: param }),
        "copy" => Ok(Shape::Copy { bytes: param }),
        other => Err(Error::UnknownShape(other.to_owned())),
    }
}

/// What `marshal_report` returns.
#[derive(Debug, Clone, Serialize)]
pub struct RunResult {
    /// The schema-valid report. This is the downloadable artefact.
    pub report: Report,
    /// Figures computed from the report. See the module docs.
    pub derived: Derived,
    /// One-line human summary of [`Derived`].
    pub description: String,
}

impl RunResult {
    /// Pairs a report with the figures divided out of it.
    #[must_use]
    pub fn of(report: Report, calls: u32) -> Self {
        let derived = Derived::of(&report, calls);
        Self {
            report,
            description: derived.describe(),
            derived,
        }
    }
}

/// Per-call and per-unit figures, divided out of the report.
///
/// Named `Derived` rather than `Measured` because that is what these are:
/// `total_ms` and `iterations` are the measurements, and everything here is
/// arithmetic on them. `None` where the question does not apply — a copy
/// baseline performs no hashes, so it has no per-hash figure and reporting
/// zero would be a claim, not an absence.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Derived {
    /// Boundary crossings in the measured loop.
    pub calls: u32,
    /// Hashes performed per crossing. 0 for the copy baseline.
    pub hashes_per_call: u32,
    /// Bytes passed per crossing.
    pub bytes_per_call: u64,
    /// `total_ms / calls`, µs.
    pub us_per_call: f64,
    /// `us_per_call / hashes_per_call`. `None` when no hashes were performed.
    pub us_per_hash: Option<f64>,
    /// `us_per_call` normalised to a KiB of payload, µs.
    pub us_per_kib: f64,
}

impl Derived {
    /// Divides the figures out of `report`.
    #[must_use]
    pub fn of(report: &Report, calls: u32) -> Self {
        let calls = calls.max(1);
        let us_per_call = report.total_ms * 1_000.0 / f64::from(calls);

        // The batch size is only in the workload id — see the crate docs for
        // why — so recovering it means reading it back out.
        let id = report.workload_id.as_str();
        let hashes_per_call = batch_of(id);
        let bytes_per_call = bytes_of(id);
        // Exact for any payload this crate can construct: `bytes_of` reads a
        // byte count that `Shape::validate` caps at 1 MiB, far inside the 53
        // bits an f64 represents exactly.
        #[allow(clippy::cast_precision_loss)]
        let kib = bytes_per_call as f64 / 1024.0;

        Self {
            calls,
            hashes_per_call,
            bytes_per_call,
            us_per_call,
            us_per_hash: if hashes_per_call > 0 {
                Some(us_per_call / f64::from(hashes_per_call))
            } else {
                None
            },
            us_per_kib: if kib > 0.0 { us_per_call / kib } else { 0.0 },
        }
    }

    /// One line naming the figure this bench exists to produce.
    #[must_use]
    pub fn describe(self) -> String {
        match self.us_per_hash {
            Some(us) => format!(
                "{} calls x {} hashes = {:.3} us/call, {:.3} us/hash",
                self.calls, self.hashes_per_call, self.us_per_call, us
            ),
            None => format!(
                "{} calls of {} B = {:.3} us/call, {:.3} us/KiB",
                self.calls, self.bytes_per_call, self.us_per_call, self.us_per_kib
            ),
        }
    }
}

/// The shape a workload id names, if it names one.
///
/// Both parsers below go through this, so the batch size and the byte count
/// come from one reading of the id and cannot disagree with each other — or
/// with what [`Shape::payload_len`] says the shape actually takes.
fn shape_of(workload_id: &str) -> Option<Shape> {
    if let Some(rest) = workload_id.strip_prefix("marshalling-poseidon-batch-") {
        return rest.parse().ok().map(|batch| Shape::Poseidon { batch });
    }
    workload_id
        .strip_prefix("marshalling-copy-")
        .and_then(|rest| rest.strip_suffix('b'))
        .and_then(|rest| rest.parse().ok())
        .map(|bytes| Shape::Copy { bytes })
}

/// The batch size a Poseidon workload id names, or 0 for anything else.
#[must_use]
pub fn batch_of(workload_id: &str) -> u32 {
    match shape_of(workload_id) {
        Some(Shape::Poseidon { batch }) => batch,
        _ => 0,
    }
}

/// The payload a workload id names, bytes, or 0 for anything else.
///
/// Applies to both shapes: a Poseidon batch of K moves `K * 64` bytes, so its
/// per-KiB figure is as meaningful as a copy baseline's. Recovering it from
/// the shape rather than from a second reading of the id is what keeps the
/// bytes-per-hash column honest.
#[must_use]
pub fn bytes_of(workload_id: &str) -> u64 {
    shape_of(workload_id).map_or(0, |shape| shape.payload_len() as u64)
}

/// What `marshal_size` returns.
#[derive(Debug, Clone, Serialize)]
pub struct PlanSummary {
    /// How many times the host should call `marshal_call`.
    pub iterations: u32,
    /// The clock resolution the decision was based on, µs.
    pub resolution_us: f64,
    /// Cost of one call, µs.
    pub per_op_us: f64,
    /// Whether `iterations` puts the loop 1000× above the resolution.
    pub target_met: bool,
    /// Whether `per_op_us` was measured rather than substituted.
    pub observed: bool,
    /// One-line human summary.
    pub description: String,
}

impl PlanSummary {
    /// Summarises a sizing decision for the page.
    #[must_use]
    pub fn of(scaling: &Scaling) -> Self {
        Self {
            iterations: scaling.iterations,
            resolution_us: scaling.resolution_us,
            per_op_us: scaling.per_op_us,
            target_met: scaling.target_met,
            observed: scaling.observed,
            description: scaling.describe(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{batch_of, bytes_of, parse_shape, Derived, RunResult, WorkloadSpec};
    use crate::shape::{self, Shape};
    use crate::Error;
    use zkprof_core::model::{Primitive, Report, System, Target};

    fn report_with(workload_id: &str, total_ms: f64) -> Report {
        Report {
            schema: None,
            report_version: "1.0".to_owned(),
            target: Target::native(),
            system: System::new("poseidon", "bn254"),
            workload_id: workload_id.to_owned(),
            total_ms,
            phases: vec![],
            primitives: vec![zkprof_core::model::PrimitiveRow {
                name: Primitive::Marshalling,
                ms: total_ms,
                share: 1.0,
            }],
            memory: zkprof_core::model::Memory::unmeasured(),
            threads: zkprof_core::model::Threads::single(),
            cross_origin_isolated: false,
            timer_resolution_us: 5.0,
            iterations: 1,
            run_manifest_ref: None,
            gpu_timing_source: None,
            backend_id: None,
            version: None,
            commitment_scheme: None,
        }
    }

    #[test]
    fn the_workload_id_parsers_round_trip_every_suite_shape() {
        for shape in shape::suite() {
            let id = shape.workload_id();

            // Both shapes carry bytes, so both must survive the round trip.
            assert_eq!(
                bytes_of(&id),
                shape.payload_len() as u64,
                "bytes lost in {id}"
            );

            match shape {
                Shape::Poseidon { batch } => {
                    assert_eq!(batch_of(&id), batch, "batch lost in {id}");
                }
                Shape::Copy { .. } => {
                    assert_eq!(batch_of(&id), 0, "copy id read as a poseidon id: {id}");
                }
            }
        }
    }

    #[test]
    fn a_poseidon_id_carries_its_payload_not_just_its_batch() {
        // The per-KiB column is only meaningful for Poseidon rows if the byte
        // count survives. Reading only the batch would leave it at zero.
        let derived = Derived::of(&report_with("marshalling-poseidon-batch-32", 100.0), 10);
        assert_eq!(derived.bytes_per_call, 32 * 64);
        assert!(
            derived.us_per_kib > 0.0,
            "a Poseidon row should have a per-KiB figure: {derived:?}"
        );
    }

    #[test]
    fn an_unparseable_id_yields_zero_rather_than_panicking() {
        assert_eq!(batch_of("not-a-workload-id"), 0);
        assert_eq!(bytes_of("not-a-workload-id"), 0);
        assert_eq!(batch_of("marshalling-poseidon-batch-"), 0);
        assert_eq!(bytes_of("marshalling-copy-b"), 0);
    }

    #[test]
    fn per_hash_is_per_call_divided_by_the_batch() {
        let derived = Derived::of(&report_with("marshalling-poseidon-batch-32", 100.0), 10);
        assert_eq!(derived.calls, 10);
        assert_eq!(derived.hashes_per_call, 32);
        assert!((derived.us_per_call - 10_000.0).abs() < 1e-6);
        assert!((derived.us_per_hash.unwrap() - 312.5).abs() < 1e-6);
    }

    #[test]
    fn a_copy_baseline_has_no_per_hash_figure() {
        let derived = Derived::of(&report_with("marshalling-copy-2048b", 20.0), 4);
        assert_eq!(derived.hashes_per_call, 0);
        assert_eq!(derived.us_per_hash, None, "a baseline performs no hashes");
        assert_eq!(derived.bytes_per_call, 2048);
        assert!((derived.us_per_kib - 2_500.0).abs() < 1e-6);
    }

    #[test]
    fn zero_calls_does_not_divide_by_zero() {
        let derived = Derived::of(&report_with("marshalling-copy-64b", 1.0), 0);
        assert_eq!(derived.calls, 1);
        assert!(derived.us_per_call.is_finite());
    }

    #[test]
    fn the_descriptions_name_the_headline_figure() {
        let poseidon = Derived::of(&report_with("marshalling-poseidon-batch-8", 8.0), 1);
        assert!(
            poseidon.describe().contains("us/hash"),
            "{}",
            poseidon.describe()
        );

        let copy = Derived::of(&report_with("marshalling-copy-512b", 8.0), 1);
        assert!(copy.describe().contains("us/KiB"), "{}", copy.describe());
    }

    #[test]
    fn every_suite_spec_describes_a_runnable_shape() {
        for shape in shape::suite() {
            let spec = WorkloadSpec::of(&shape);
            assert_eq!(spec.shape().expect("a runnable shape"), shape);
            assert_eq!(spec.payload_len, shape.payload_len());
            assert!(!spec.label.is_empty());
        }
    }

    #[test]
    fn the_suite_specs_alternate_poseidon_with_its_copy_baseline() {
        let specs = WorkloadSpec::suite();
        assert!(specs.len() >= 4, "the suite should have several workloads");
        let (pairs, remainder) = specs.as_chunks::<2>();
        assert!(remainder.is_empty(), "the suite should pair up exactly");
        for pair in pairs {
            assert_eq!(pair[0].kind, "poseidon", "{pair:?}");
            assert_eq!(pair[1].kind, "copy", "{pair:?}");
            assert_eq!(
                pair[0].payload_len, pair[1].payload_len,
                "the copy baseline should carry the same bytes as the hashes it stands in for"
            );
        }
    }

    #[test]
    fn an_unknown_shape_is_refused_by_name() {
        let error = parse_shape("sha3", 1).expect_err("sha3 is not a shape");
        assert!(matches!(error, Error::UnknownShape(ref kind) if kind == "sha3"));
        assert!(error.to_string().contains("sha3"), "{error}");
    }

    #[test]
    fn a_run_result_carries_the_report_and_its_own_derived_block() {
        let report = report_with("marshalling-poseidon-batch-8", 8.0);
        let result = RunResult::of(report.clone(), 4);
        assert_eq!(result.report.workload_id, "marshalling-poseidon-batch-8");
        assert_eq!(result.derived.calls, 4);
        assert!(
            result.description.contains("us/hash"),
            "{}",
            result.description
        );
    }
}
