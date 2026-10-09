// SPDX-License-Identifier: Apache-2.0

//! What a dispatch costs, measured two ways.
//!
//! # The two numbers
//!
//! A WebGPU dispatch can be timed from the host or from the device, and the
//! two are not estimates of one quantity — they are measurements of two
//! different ones:
//!
//! * **wall-clock** — the host clock, spanning the moment the command buffer
//!   is submitted to the moment the device reports the submission finished.
//!   It contains queue latency, submission overhead, and everything else in
//!   the round trip.
//! * **gpu-pass** — a `GPUQuerySet` timestamp written at the start of the
//!   compute pass and another at its end. It contains the pass.
//!
//! # The limitation, stated once and reused verbatim
//!
//! The timestamp-query figure is **pass-level only; copies/resource-creation/
//! submit overhead are invisible to it (that is precisely why both numbers
//! ship)**.
//!
//! Spelled out, because each clause rules something out:
//!
//! * *pass-level only* — the two timestamps bracket the compute pass. The
//!   `copy_buffer_to_buffer` that moves the product into the staging buffer
//!   happens after the pass ends and is therefore outside both numbers'
//!   subject: invisible to the query, and still inside the wall clock.
//! * *copies* — any buffer-to-buffer copy on the encoder, including the
//!   copy-out, is not a compute pass and gets no timestamps.
//! * *resource-creation* — allocating the query set, the resolve buffer and
//!   the readback buffer costs host time and is absent from the pass figure.
//! * *submit overhead* — encoding the command buffer and handing it to the
//!   queue happens before the first timestamp is written, so it is in the
//!   wall clock and not in the pass.
//!
//! So the pass figure is *narrower*, not *better*: it answers "how long did
//! the device spend on this pass", where the wall clock answers "how long did
//! this dispatch cost the host". Neither is a substitute for the other, and
//! the gap between them is the overhead — which is why a run that has the
//! feature reports both, and a run that does not says which instrument it
//! used.
//!
//! # Why this module is native
//!
//! None of it touches `wgpu`. The arithmetic that turns two counter readings
//! into a duration is exactly the kind of thing that should fail a unit test
//! rather than a browser session, and [`crate::pipeline`] cannot be reached by
//! one — see the crate docs.

use std::mem::size_of;

use serde::Serialize;
use zkprof_core::model::GpuTimingSource;

/// Phase name under which the GPU-pass figure is reported.
///
/// Distinct from [`crate::run::DISPATCH`], which keeps the wall clock. Two
/// names because collapsing them would leave one number with no way to say
/// which instrument produced it.
pub const GPU_PASS_PHASE: &str = "dispatch-gpu-pass";

/// Timestamp slots in the query set: one for the start of the pass, one for
/// its end.
pub const TIMESTAMP_SLOTS: u32 = 2;

/// Bytes one timestamp slot occupies in a resolved buffer.
///
/// `QUERY_SIZE` in `wgpu`, which is 8. Spelled out rather than imported so
/// this module stays `wgpu`-free and native-testable.
pub const SLOT_BYTES: u64 = 8;

/// Bytes the whole query set resolves into: two slots.
///
/// Held in step with [`TIMESTAMP_SLOTS`] by a test rather than by arithmetic,
/// because `u64::from` is not const and every spelling of the widening trips
/// a pedantic cast lint. A test says the same thing and can explain itself.
pub const RESOLVED_BYTES: u64 = 16;

/// Nanoseconds per millisecond.
const NS_PER_MS: f64 = 1_000_000.0;

/// One dispatch, timed both ways.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct DispatchTiming {
    /// Host clock, submission to completion, milliseconds.
    pub wall_clock_ms: f64,

    /// Device timestamp delta across the compute pass, milliseconds.
    ///
    /// `None` when the adapter did not offer `timestamp-query`, or when the
    /// query resolved to something that is not a duration. Absence here is
    /// never reported as `0.0`: a zero would read as a pass that took no
    /// time, which is a different claim from "not measured".
    pub gpu_pass_ms: Option<f64>,
}

impl DispatchTiming {
    /// A dispatch with only the host figure, because the device offered no
    /// timestamp query.
    #[must_use]
    pub fn wall_clock_only(wall_clock_ms: f64) -> Self {
        Self {
            wall_clock_ms,
            gpu_pass_ms: None,
        }
    }

    /// Attaches the device figure.
    ///
    /// Rejects a pass figure that is not finite or not positive-and-sane,
    /// rather than publishing it: a negative or `NaN` duration would fail
    /// report validation further down with a message that no longer says
    /// which instrument broke.
    #[must_use]
    pub fn with_gpu_pass_ms(mut self, gpu_pass_ms: f64) -> Self {
        if gpu_pass_ms.is_finite() && gpu_pass_ms >= 0.0 {
            self.gpu_pass_ms = Some(gpu_pass_ms);
        }
        self
    }

    /// Which instrument produced this run's GPU figure.
    #[must_use]
    pub fn source(&self) -> GpuTimingSource {
        if self.gpu_pass_ms.is_some() {
            GpuTimingSource::TimestampQuery
        } else {
            GpuTimingSource::WallClock
        }
    }

    /// The figure the report attributes to the primitive.
    ///
    /// The device figure when there is one — it is the narrower and more
    /// direct measurement, and the wall clock counts queue and submit
    /// overhead as if it were field arithmetic. The wall clock otherwise,
    /// which is exactly what W3.5 reported and keeps that figure's meaning
    /// intact on hardware without the feature.
    #[must_use]
    pub fn attributed_ms(&self) -> f64 {
        self.gpu_pass_ms.unwrap_or(self.wall_clock_ms)
    }

    /// Wall-clock time the device figure does not account for, milliseconds.
    ///
    /// `None` when there is no device figure to compare against. This is the
    /// overhead — submission latency, resource creation, the copy-out — and
    /// it is a derived quantity shown to a reader, never recorded as a span.
    #[must_use]
    pub fn overhead_ms(&self) -> Option<f64> {
        self.gpu_pass_ms
            .map(|pass| (self.wall_clock_ms - pass).max(0.0))
    }
}

/// Duration between two resolved timestamps, in nanoseconds.
///
/// `period_ns` is [`wgpu::Queue::get_timestamp_period`]'s value: one tick is
/// that many nanoseconds. Returns `None` when the pair does not describe a
/// duration.
///
/// # Why `wrapping_sub`
///
/// WebGPU timestamp counters are unsigned and may wrap to zero mid-run. A
/// plain subtraction across a wrap yields a huge positive number; the usual
/// advice about "spurious negative deltas" comes from reading the pair as
/// signed. `wrapping_sub` on `u64` recovers the true elapsed count as long as
/// the pass lasted under 2^64 ns, which is 584 years.
///
/// # Why a pair of zeros is `None`
///
/// A slot the device never wrote reads back as `0`. Two zeros are not a
/// zero-length pass — they are an absence. Reporting `0.0` would publish a
/// measurement that did not happen, which is the one thing this crate's
/// checks exist to prevent.
#[must_use]
pub fn timestamp_delta_ns(begin: u64, end: u64, period_ns: f32) -> Option<f64> {
    if !period_ns.is_finite() || period_ns <= 0.0 {
        return None;
    }
    if begin == 0 && end == 0 {
        return None;
    }
    let ticks = end.wrapping_sub(begin);
    // A count of nanoseconds stays exact in an f64 until 2^53 ns, which is
    // 104 days. A pass that long has failed in a way a duration will not
    // describe.
    #[allow(clippy::cast_precision_loss)]
    let ns = ticks as f64 * f64::from(period_ns);
    Some(ns)
}

/// Nanoseconds to milliseconds.
#[must_use]
pub fn ns_to_ms(ns: f64) -> f64 {
    ns / NS_PER_MS
}

/// The two timestamps a resolved query set wrote, as `(begin, end)`.
///
/// Returns `None` if fewer than 16 bytes were handed in, which for a `MAP_READ`
/// buffer of exactly [`RESOLVED_BYTES`] means the readback itself was short.
#[must_use]
pub fn read_timestamps(bytes: &[u8]) -> Option<(u64, u64)> {
    let (begin_bytes, rest) = bytes.split_at_checked(size_of::<u64>())?;
    let (end_bytes, _) = rest.split_at_checked(size_of::<u64>())?;
    Some((
        u64::from_le_bytes(begin_bytes.try_into().ok()?),
        u64::from_le_bytes(end_bytes.try_into().ok()?),
    ))
}

#[cfg(test)]
mod tests {
    use super::{
        ns_to_ms, read_timestamps, timestamp_delta_ns, DispatchTiming, GPU_PASS_PHASE,
        RESOLVED_BYTES, SLOT_BYTES, TIMESTAMP_SLOTS,
    };
    use zkprof_core::model::GpuTimingSource;

    #[test]
    fn the_two_phase_names_are_different_strings() {
        // The whole point of the second phase is that it is not the first one.
        assert_ne!(GPU_PASS_PHASE, crate::run::DISPATCH);
        assert_eq!(GPU_PASS_PHASE, "dispatch-gpu-pass");
    }

    #[test]
    fn two_slots_hold_two_timestamps() {
        assert_eq!(TIMESTAMP_SLOTS, 2);
        assert_eq!(RESOLVED_BYTES, 16);
        assert_eq!(SLOT_BYTES, 8);
    }

    #[test]
    fn a_plain_delta_is_the_difference_times_the_period() {
        // 12 345 ticks at 1 ns each is 12.345 µs.
        let ns = timestamp_delta_ns(1000, 11_345, 1.0).expect("a duration");
        assert!((ns - 10_345.0).abs() < 1e-9, "{ns}");
        assert!((ns_to_ms(ns) - 0.010_345).abs() < 1e-12);
    }

    #[test]
    fn the_period_scales_the_result() {
        // Dawn-era hardware has reported periods above 1 ns.
        let at_one = timestamp_delta_ns(0, 100, 1.0).expect("a duration");
        let at_four = timestamp_delta_ns(0, 100, 4.0).expect("a duration");
        assert!((at_four - at_one * 4.0).abs() < 1e-9);
    }

    #[test]
    fn a_counter_wrap_still_yields_the_elapsed_time() {
        // `end` has wrapped past zero. Reading the pair as signed gives a
        // large negative number; `wrapping_sub` gives the 100 ticks that
        // actually elapsed.
        let begin = u64::MAX - 49;
        let end = 50;
        assert_eq!(timestamp_delta_ns(begin, end, 1.0), Some(100.0));
    }

    #[test]
    fn two_zeros_are_an_absence_not_a_zero_length_pass() {
        assert_eq!(timestamp_delta_ns(0, 0, 1.0), None);
    }

    #[test]
    fn a_non_positive_period_is_not_a_duration() {
        assert_eq!(timestamp_delta_ns(5, 10, 0.0), None);
        assert_eq!(timestamp_delta_ns(5, 10, -1.0), None);
        assert_eq!(timestamp_delta_ns(5, 10, f32::NAN), None);
    }

    #[test]
    fn timestamps_are_read_back_little_endian() {
        let mut bytes = [0u8; 16];
        bytes[..8].copy_from_slice(&1000u64.to_le_bytes());
        bytes[8..].copy_from_slice(&11_345u64.to_le_bytes());

        assert_eq!(read_timestamps(&bytes), Some((1000, 11_345)));
    }

    #[test]
    fn a_short_readback_yields_nothing() {
        assert_eq!(read_timestamps(&[0u8; 15]), None);
        assert_eq!(read_timestamps(&[]), None);
    }

    #[test]
    fn a_wall_clock_only_timing_says_so() {
        let timing = DispatchTiming::wall_clock_only(4.0);
        assert_eq!(timing.gpu_pass_ms, None);
        assert_eq!(timing.source(), GpuTimingSource::WallClock);
        // With no device figure, the wall clock is what gets attributed --
        // the same thing W3.5 always reported.
        assert!((timing.attributed_ms() - 4.0).abs() < 1e-12);
        assert_eq!(timing.overhead_ms(), None);
    }

    #[test]
    fn a_device_figure_replaces_the_wall_clock_in_the_attribution() {
        let timing = DispatchTiming::wall_clock_only(4.0).with_gpu_pass_ms(1.5);
        assert_eq!(timing.source(), GpuTimingSource::TimestampQuery);
        assert!((timing.attributed_ms() - 1.5).abs() < 1e-12);
        // 2.5 ms of the 4 ms wall clock is not the pass: submission latency,
        // resource creation, the copy-out.
        assert!((timing.overhead_ms().unwrap() - 2.5).abs() < 1e-12);
    }

    #[test]
    fn a_pass_longer_than_the_wall_clock_clamps_the_overhead_to_zero() {
        // Two instruments disagreeing is not a negative overhead. It happens:
        // the clocks are different clocks, and one of them is coarse.
        let timing = DispatchTiming::wall_clock_only(1.0).with_gpu_pass_ms(1.5);
        assert!((timing.overhead_ms().unwrap() - 0.0).abs() < 1e-12);
    }

    #[test]
    fn an_unusable_device_figure_is_dropped_not_published() {
        // NaN or negative would fail report validation later with a message
        // that no longer names the instrument at fault.
        let timing = DispatchTiming::wall_clock_only(4.0).with_gpu_pass_ms(f64::NAN);
        assert_eq!(timing.gpu_pass_ms, None);
        assert_eq!(timing.source(), GpuTimingSource::WallClock);

        let timing = DispatchTiming::wall_clock_only(4.0).with_gpu_pass_ms(-1.0);
        assert_eq!(timing.gpu_pass_ms, None);
    }

    #[test]
    fn a_zero_length_pass_is_a_real_measurement() {
        // Distinct from the readback never having been written: here the
        // device did write, and it wrote the same value twice.
        let timing = DispatchTiming::wall_clock_only(4.0).with_gpu_pass_ms(0.0);
        assert_eq!(timing.gpu_pass_ms, Some(0.0));
        assert_eq!(timing.source(), GpuTimingSource::TimestampQuery);
    }
}
