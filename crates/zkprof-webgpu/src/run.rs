// SPDX-License-Identifier: Apache-2.0

//! What a run is divided into, and what it hands back.
//!
//! # The five phases
//!
//! A WebGPU run is not one thing that takes time; it is a sequence of five
//! things that are separately interesting and separately fallible. Measuring
//! only the total would tell a reader that the plumbing took 40 ms and not
//! whether that was acquiring the device, compiling the shader, running the
//! kernel, or waiting for the readback.
//!
//! # What `field_arith` does and does not mean here
//!
//! Only the `dispatch` window is attributed to a primitive, and it is
//! [`zkprof_core::Primitive::FieldArith`] because M31 multiplication is field
//! arithmetic.
//!
//! What that figure *is* depends on the hardware, and the report says which:
//!
//! * With `timestamp-query` — a `GPUQuerySet` brackets the compute pass, and
//!   the attributed figure is the resolved timestamp delta. That is the pass,
//!   and little else.
//! * Without it — the window opens when the command buffer is submitted and
//!   closes when the device reports the submission finished, so the figure is
//!   an **upper bound** on the kernel rather than a measurement of it: it
//!   contains submission latency and the copy-out to the staging buffer
//!   alongside the kernel itself. This is what W3.5 always reported.
//!
//! `timestamp-query` is an optional WebGPU feature that may not be present,
//! so a run reports both figures when it can and labels them separately:
//! [`DISPATCH`] keeps the wall clock, [`crate::timing::GPU_PASS_PHASE`] adds
//! the pass. See [`crate::timing`] for what the pass figure cannot see, which
//! is the reason both ship.
//!
//! Everything else — device acquisition, upload, compilation, readback — is
//! plumbing, and stays out of the primitive rows. It shows up in
//! [`zkprof_core::Report::unattributed_ms`], which is where this repository
//! puts time that was measured but not attributed.

use serde::Serialize;
use zkprof_core::model::{GpuTimingSource, Primitive, Report};
use zkprof_core::scale::TARGET_RATIO;

use crate::spec::Spec;
use crate::timing::DispatchTiming;

/// Instance and adapter request, then device and queue request.
pub const DEVICE_ACQUIRE: &str = "device-acquire";

/// Buffer creation and the two `write_buffer` calls that fill the operands.
pub const UPLOAD: &str = "upload";

/// Shader module, bind group layout, pipeline layout, compute pipeline and
/// bind group. Shader compilation is frequently the slowest step in a first
/// run, which is exactly why it is not folded into `device-acquire`.
pub const COMPILE: &str = "compile";

/// Command encoding, submission, and the wait for the device to report the
/// submission finished. Includes the copy-out to the staging buffer.
pub const DISPATCH: &str = "dispatch";

/// Mapping the staging buffer and reading it into host memory.
pub const READBACK: &str = "readback";

/// Every plumbing phase, in the order they run.
///
/// Five, and deliberately not six: [`crate::timing::GPU_PASS_PHASE`] is not a
/// step in the sequence but a second measurement of one of them, present only
/// when the device has `timestamp-query`. Folding it in here would make the
/// count hardware-dependent and the list a lie on machines without it.
pub const PHASES: [&str; 5] = [DEVICE_ACQUIRE, UPLOAD, COMPILE, DISPATCH, READBACK];

/// The only primitive a plumbing run attributes time to.
pub const PRIMITIVE: Primitive = Primitive::FieldArith;

/// One run's result, for the page to render.
///
/// An envelope rather than a bare report, for the same reason the harness
/// page gets one: everything the page would otherwise recompute in JavaScript
/// is computed here instead, from the same values the report was built from.
/// `below_target` in particular is a rule about the clock, and duplicating it
/// in JavaScript is how a page ends up disagreeing with the CLI about whether
/// a row is above the noise floor.
#[derive(Debug, Clone, Serialize)]
pub struct RunResult {
    /// The schema-valid report. The only part meant to leave the page.
    pub report: Report,

    /// The adapter string, the same one in `report.target.gpu`.
    ///
    /// Duplicated because the page shows it above the fold next to the
    /// browser and OS, and reading it back out of `target.gpu` would mean
    /// handling the case where it is absent twice.
    pub adapter: String,

    /// What was run: element count, dispatch count, seed.
    pub spec: Spec,

    /// Whether the device's output matched the CPU reference.
    ///
    /// Always `true`, by construction: a run whose output disagrees with
    /// [`Spec::expected`] returns an [`crate::Error::Mismatch`] instead of a
    /// report. It is carried anyway so the page can state that the check
    /// happened, rather than leaving a reader to wonder whether it did.
    pub verified: bool,

    /// Bytes handed to the device: both operand buffers.
    pub bytes_up: u64,

    /// Bytes read back: the product buffer.
    pub bytes_down: u64,

    /// Primitive rows whose accumulated time is under [`TARGET_RATIO`] times
    /// the measured clock resolution.
    ///
    /// Empty is the good case. A name here is not an error, but it must not
    /// be compared against anything.
    pub below_target: Vec<String>,

    /// Workgroups launched per dispatch.
    pub workgroups_per_dispatch: u32,

    /// The dispatch, measured both ways.
    ///
    /// Carried whole rather than as two loose fields so the page cannot print
    /// one figure under the other's label by accident. `gpu_pass_ms` is
    /// `None` on hardware without `timestamp-query`, and the page is expected
    /// to say so rather than to print a dash that could be read as a zero.
    pub timing: DispatchTiming,
}

impl RunResult {
    /// Assembles the envelope around a finished report.
    #[must_use]
    pub fn new(report: Report, adapter: String, spec: Spec, timing: DispatchTiming) -> Self {
        let below_target = below_target(&report);
        Self {
            report,
            adapter,
            bytes_up: spec.buffer_bytes() * 2,
            bytes_down: spec.buffer_bytes(),
            workgroups_per_dispatch: spec.workgroups(),
            spec,
            verified: true,
            below_target,
            timing,
        }
    }

    /// Which instrument produced the GPU figure in `report`.
    ///
    /// The same value as `report.gpu_timing_source`, duplicated for the page's
    /// benefit for the same reason `adapter` is: reading it back out of the
    /// report means handling the absent case twice.
    #[must_use]
    pub fn gpu_timing_source(&self) -> Option<GpuTimingSource> {
        self.report.gpu_timing_source
    }
}

/// Primitive rows that did not clear the noise floor.
///
/// The same rule the arkworks backend uses, so a row is below target here on
/// exactly the terms it is below target there: accumulated time under
/// [`TARGET_RATIO`] × the measured resolution × the iteration count.
#[must_use]
pub fn below_target(report: &Report) -> Vec<String> {
    let floor_us = TARGET_RATIO * report.timer_resolution_us * f64::from(report.iterations);
    report
        .primitives
        .iter()
        .filter(|row| row.ms * 1_000.0 < floor_us)
        .map(|row| row.name.as_str().to_owned())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{below_target, RunResult, DISPATCH, PHASES, PRIMITIVE};
    use crate::adapter::webgpu_target;
    use crate::spec::{system, Spec};
    use crate::timing::{DispatchTiming, GPU_PASS_PHASE};
    use zkprof_core::model::{GpuTimingSource, Target, TargetKind};
    use zkprof_core::profiler::Profiler;
    use zkprof_core::{platform_clock, Label};

    /// Durations chosen to be far above any plausible clock resolution, so
    /// the structural assertions below do not depend on the host's timer.
    const PHASE_US: [f64; 5] = [40_000.0, 2_000.0, 30_000.0, 5_000.0, 3_000.0];

    fn target() -> Target {
        webgpu_target(
            Target {
                kind: TargetKind::WasmScalar,
                browser: Some("Chrome 141".to_owned()),
                os: "Mac OS X".to_owned(),
                cpu: "wasm32".to_owned(),
                gpu: None,
            },
            "Apple M1 Pro [Metal / DiscreteGpu]",
        )
    }

    /// Builds a report the way the pipeline does, from recorded durations.
    ///
    /// `set_region_ms` pins the total so the arithmetic below is exact: with
    /// a real clock the region between construction and finish is whatever
    /// the host made of it, which is not a number a test can assert on. The
    /// pipeline does not call it, because there the region is the real thing
    /// being measured.
    fn synthesized() -> zkprof_core::model::Report {
        let clock = platform_clock().expect("a clock");
        let spec = Spec::default().with_dispatches(4);
        let profiler = Profiler::new(clock.as_ref(), target(), system(), spec.workload_id())
            .with_iterations(spec.dispatches);

        for (name, us) in PHASES.iter().zip(PHASE_US) {
            profiler.record(Label::Phase((*name).to_owned()), us);
        }
        profiler.record(PRIMITIVE, PHASE_US[3]);
        profiler.set_region_ms(PHASE_US.iter().sum::<f64>() / 1_000.0);

        profiler.finish().expect("the report must assemble")
    }

    #[test]
    fn the_phase_list_is_in_the_order_the_run_performs_them() {
        assert_eq!(PHASES[0], "device-acquire");
        assert_eq!(PHASES[4], "readback");
        assert_eq!(PHASES.len(), 5);
        assert!(PHASES.contains(&DISPATCH));
    }

    #[test]
    fn the_report_carries_every_phase_and_nothing_else() {
        let report = synthesized();

        let names: Vec<&str> = report
            .phases
            .iter()
            .map(|phase| phase.name.as_str())
            .collect();
        assert_eq!(names, PHASES.to_vec());
    }

    #[test]
    fn only_the_dispatch_window_is_attributed_to_a_primitive() {
        let report = synthesized();

        assert_eq!(report.primitives.len(), 1, "{:?}", report.primitives);
        assert_eq!(report.primitives[0].name, PRIMITIVE);
        assert!((report.primitives[0].share - 1.0).abs() < 1e-12);
        // 5 000 µs recorded, in milliseconds.
        assert!((report.primitives[0].ms - 5.0).abs() < 1e-9);
    }

    #[test]
    fn the_plumbing_stays_unattributed_rather_than_being_a_primitive() {
        // 80 ms of region, 5 ms of which is the kernel. The remaining 75 ms
        // is device acquisition, upload, compilation and readback: measured,
        // real, and deliberately not called field arithmetic.
        let report = synthesized();
        assert!((report.unattributed_ms() - 75.0).abs() < 1e-9);
    }

    #[test]
    fn the_target_is_a_webgpu_target_that_names_the_adapter() {
        let report = synthesized();
        assert_eq!(report.target.kind, TargetKind::Webgpu);
        assert_eq!(
            report.target.gpu.as_deref(),
            Some("Apple M1 Pro [Metal / DiscreteGpu]")
        );
    }

    #[test]
    fn the_report_validates_and_carries_the_dispatch_count_as_iterations() {
        let report = synthesized();
        report.validate().expect("the report must validate");
        assert_eq!(report.iterations, 4);
        assert_eq!(report.workload_id, "webgpu-m31-mul-65536-x4");
    }

    #[test]
    fn a_row_above_the_noise_floor_is_not_flagged() {
        let report = synthesized();
        assert!(
            below_target(&report).is_empty(),
            "{:?} with a resolution of {} µs",
            below_target(&report),
            report.timer_resolution_us
        );
    }

    #[test]
    fn a_row_below_the_noise_floor_is_flagged_by_name() {
        // One microsecond of kernel against a 100 µs clock is 10 000× too
        // small to have been resolved. `iterations` is 1, so the floor is
        // 100 000 µs.
        let clock = platform_clock().expect("a clock");
        let profiler = Profiler::new(clock.as_ref(), target(), system(), "webgpu-m31-mul-1-x1");
        profiler.record(PRIMITIVE, 1.0);
        profiler.record(Label::Phase(DISPATCH.to_owned()), 1.0);
        profiler.set_region_ms(1.0);

        let mut report = profiler.finish().expect("assembles");
        report.timer_resolution_us = 100.0;

        assert_eq!(below_target(&report), vec![PRIMITIVE.as_str()]);
    }

    #[test]
    fn the_envelope_reports_the_bytes_the_run_moved() {
        let spec = Spec::default().with_elements(1024).with_dispatches(2);
        let report = synthesized();
        let result = RunResult::new(
            report,
            "adapter".to_owned(),
            spec,
            DispatchTiming::wall_clock_only(5.0),
        );

        // 4 KiB per buffer: two up, one down.
        assert_eq!(result.bytes_up, 8192);
        assert_eq!(result.bytes_down, 4096);
        assert_eq!(result.workgroups_per_dispatch, 16);
        assert!(result.verified);
        assert!(result.below_target.is_empty());
    }

    #[test]
    fn the_envelope_serialises_for_the_page() {
        let result = RunResult::new(
            synthesized(),
            "adapter".to_owned(),
            Spec::default(),
            DispatchTiming::wall_clock_only(5.0),
        );
        let json = serde_json::to_string(&result).expect("serialises");

        // The report has to survive as an object the page can hand straight
        // to a download, not as an escaped string.
        let value: serde_json::Value = serde_json::from_str(&json).expect("parses");
        assert_eq!(
            value["report"]["target"]["kind"],
            serde_json::json!("webgpu")
        );
        assert_eq!(value["spec"]["elements"], serde_json::json!(65536));
    }

    #[test]
    fn the_envelope_carries_both_dispatch_figures() {
        // The page prints these two side by side and must not be able to
        // confuse them, so they travel as one object with named fields.
        let result = RunResult::new(
            synthesized(),
            "adapter".to_owned(),
            Spec::default(),
            DispatchTiming::wall_clock_only(5.0).with_gpu_pass_ms(1.5),
        );

        assert!((result.timing.wall_clock_ms - 5.0).abs() < 1e-12);
        assert!((result.timing.gpu_pass_ms.unwrap() - 1.5).abs() < 1e-12);
        assert_eq!(
            result.timing.source(),
            GpuTimingSource::TimestampQuery,
            "the envelope must name the instrument, not leave the page to infer it"
        );
    }

    #[test]
    fn the_gpu_pass_phase_is_a_measurement_not_a_sixth_plumbing_step() {
        // `PHASES` is the sequence of things that happen; `GPU_PASS_PHASE` is
        // a second look at one of them. Keeping them apart is what lets the
        // phase list mean the same thing on hardware with and without the
        // feature.
        assert!(!PHASES.contains(&GPU_PASS_PHASE));
        assert!(PHASES.contains(&DISPATCH));
    }
}
