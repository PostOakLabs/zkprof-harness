// SPDX-License-Identifier: Apache-2.0

//! End-to-end behaviour of the crate as a caller sees it: open a profiler,
//! run some spans, emit JSON, read it back, validate it.
//!
//! These are integration tests on purpose. The unit tests inside the crate use
//! a scripted clock and can therefore only prove the arithmetic; these use the
//! real native clock and prove the pieces fit together.

use std::cell::Cell;

use zkprof_core::clock::{probe_resolution, Clock, Resolution, DEFAULT_PROBE_SAMPLES};
use zkprof_core::model::{Primitive, Report, System, Target, Threads};
use zkprof_core::profiler::Profiler;
use zkprof_core::{plan_iterations, scale_iterations, SCHEMA_ID};

/// A clock that advances in fixed steps, so the pipeline can be checked for
/// exact values while still exercising the real code path.
#[derive(Debug)]
struct ScriptedClock {
    now_us: Cell<f64>,
}

impl ScriptedClock {
    fn new(start_us: f64) -> Self {
        Self {
            now_us: Cell::new(start_us),
        }
    }

    fn advance(&self, us: f64) {
        self.now_us.set(self.now_us.get() + us);
    }
}

impl Clock for ScriptedClock {
    fn now_us(&self) -> f64 {
        self.now_us.get()
    }

    fn cross_origin_isolated(&self) -> bool {
        false
    }

    fn name(&self) -> &'static str {
        "scripted"
    }
}

const RESOLUTION: Resolution = Resolution::Measured {
    min_nonzero_delta_us: 5.0,
    samples: DEFAULT_PROBE_SAMPLES,
};

fn burn(clock: &ScriptedClock, us: f64) {
    clock.advance(us);
}

#[test]
fn a_report_survives_the_full_json_round_trip() {
    let clock = ScriptedClock::new(1_000.0);
    let profiler = Profiler::new(
        &clock,
        Target::native(),
        System::new("groth16", "bn254").with_constraint_count(16_384),
        "pipeline-fixture",
    )
    .with_resolution(RESOLUTION)
    .with_iterations(16)
    .with_threads(Threads::single());

    profiler.span("setup", || burn(&clock, 1_000.0));
    profiler.span(Primitive::WitnessGen, || burn(&clock, 2_000.0));
    profiler.span(Primitive::Msm, || burn(&clock, 5_000.0));
    profiler.span(Primitive::NttFft, || burn(&clock, 2_000.0));
    profiler.span(Primitive::Other, || burn(&clock, 1_000.0));

    let report = profiler.finish().expect("the report should build");
    report.validate().expect("the report should validate");

    let json = report
        .to_json_pretty()
        .expect("serialisation should succeed");
    let reparsed = Report::from_json(&json).expect("the JSON should parse back");

    assert_eq!(reparsed, report, "the round trip must be lossless");
    reparsed
        .validate()
        .expect("the reparsed report should validate");
}

#[test]
fn the_json_uses_the_schema_field_names_not_the_rust_ones() {
    let clock = ScriptedClock::new(0.0);
    let profiler = Profiler::new(
        &clock,
        Target::native(),
        System::new("plonk", "bls12-381"),
        "field-names",
    )
    .with_resolution(RESOLUTION);

    profiler.span(Primitive::Hash, || burn(&clock, 1_000.0));

    let report = profiler.finish().expect("the report should build");
    let value: serde_json::Value =
        serde_json::from_str(&report.to_json().expect("serialisation")).unwrap();

    // camelCase where the schema says so, snake_case where it says so.
    assert!(value.get("crossOriginIsolated").is_some());
    assert!(value.get("cross_origin_isolated").is_none());
    assert!(value.get("timer_resolution_us").is_some());
    assert_eq!(value["$schema"], SCHEMA_ID);
}

#[test]
fn primitive_shares_partition_the_attributed_time() {
    let clock = ScriptedClock::new(0.0);
    let profiler = Profiler::new(
        &clock,
        Target::native(),
        System::new("groth16", "bn254"),
        "shares",
    )
    .with_resolution(RESOLUTION);

    profiler.span(Primitive::Msm, || burn(&clock, 6_000.0));
    profiler.span(Primitive::NttFft, || burn(&clock, 3_000.0));
    profiler.span(Primitive::Other, || burn(&clock, 1_000.0));

    let report = profiler.finish().expect("the report should build");

    let sum: f64 = report.primitives.iter().map(|row| row.share).sum();
    assert!((sum - 1.0).abs() < 1e-9, "shares summed to {sum}");
    assert!((report.primitive(Primitive::Msm).unwrap().share - 0.6).abs() < 1e-9);
    assert!((report.primitive(Primitive::NttFft).unwrap().share - 0.3).abs() < 1e-9);
    assert!((report.primitive(Primitive::Other).unwrap().share - 0.1).abs() < 1e-9);
}

#[test]
fn time_outside_every_span_stays_visible_as_unattributed() {
    let clock = ScriptedClock::new(0.0);
    let profiler = Profiler::new(
        &clock,
        Target::native(),
        System::new("groth16", "bn254"),
        "unattributed",
    )
    .with_resolution(RESOLUTION);

    profiler.span(Primitive::Msm, || burn(&clock, 4_000.0));
    burn(&clock, 1_000.0);

    let report = profiler.finish().expect("the report should build");
    assert!((report.total_ms - 5.0).abs() < 1e-9);
    assert!((report.unattributed_ms() - 1.0).abs() < 1e-9);
}

#[test]
fn the_native_clock_produces_a_plausible_resolution_and_a_usable_plan() {
    let clock = zkprof_core::clock::platform_clock().expect("a native clock");
    let resolution = probe_resolution(clock.as_ref(), DEFAULT_PROBE_SAMPLES);

    assert!(
        resolution.observed(),
        "the native clock must advance during the probe"
    );
    assert!(resolution.lower_bound_us() > 0.0);

    // A workload with a real cost must plan to a met target.
    let mut sink = 0_u64;
    let plan = plan_iterations(clock.as_ref(), || {
        sink = sink.wrapping_add(1).wrapping_mul(6_364_136_223_235_839);
        std::hint::black_box(sink);
    });

    assert!(
        plan.scaling.iterations >= 1,
        "the plan must call the workload at least once"
    );
    // One warm-up call plus at least one timed call.
    assert!(plan.calls_spent >= 2);
}

#[test]
fn scaling_never_returns_a_count_outside_the_legal_range() {
    // The schema requires iterations >= 1 and the report requires a count that
    // fits in a u32; sweep the corners of the input space and check both.
    let durations = [0.0, 1e-9, 1.0, 1e3, 1e6, f64::NAN, f64::INFINITY];
    let resolutions = [0.0, 1e-9, 0.005, 5.0, 100.0, f64::NAN, f64::INFINITY];

    for measured in durations {
        for count in [1_u32, 10, 1_000, u32::MAX] {
            for resolution in resolutions {
                let scaling = scale_iterations(measured, count, resolution);
                assert!(scaling.iterations >= 1);
                assert!(scaling.iterations <= zkprof_core::MAX_ITERATIONS);
                assert!(scaling.achieved_ratio.is_finite());
            }
        }
    }
}
