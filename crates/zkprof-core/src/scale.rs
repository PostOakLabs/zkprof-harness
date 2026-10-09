// SPDX-License-Identifier: Apache-2.0

//! Iteration auto-scaling: how many times to repeat a workload so the clock
//! can actually see it.
//!
//! # The rule
//!
//! Each measured phase must span at least [`TARGET_RATIO`]× the measured clock
//! resolution. At 1000× the quantisation noise is ~0.1%, which is below every
//! other source of variance in a browser timing run.
//!
//! # Why the API takes a batch, not a single shot
//!
//! An earlier shape of this module took a single-shot duration. It did not
//! work, and the reason is worth recording: a single op that lands below the
//! clock resolution measures as **zero**, and the only defensible estimate from
//! a zero is "somewhere in `(0, resolution]`". Substituting the resolution
//! there makes the required count come out at exactly [`TARGET_RATIO`]
//! regardless of how fast the op really is — so [`MAX_ITERATIONS`] could never
//! be reached and `target_met` could never be `false`.
//!
//! The question is therefore answered from a **measured batch**:
//! [`scale_iterations`] takes how long `measured_count` iterations took, and
//! [`plan_iterations`] drives it, re-measuring with a larger batch until the
//! batch itself registers. That converges in two or three rounds for any
//! workload the clock can see at all, and terminates at the cap for one it
//! cannot.
//!
//! # What happens when the target cannot be reached
//!
//! [`Scaling::target_met`] is `false`. That is a signal to widen the workload
//! or report the phase as below the noise floor — never to drop the number
//! into a report as if it were resolved.

use crate::clock::{probe_resolution, Clock, Resolution, DEFAULT_PROBE_SAMPLES};

/// How many times coarser than the clock a measured phase must be.
pub const TARGET_RATIO: f64 = 1_000.0;

/// Upper bound on an auto-scaled iteration count.
///
/// 10⁶ iterations of anything that takes even 1 µs is a second of work. Past
/// this point the honest move is to profile a bigger workload, not to run the
/// small one more times.
pub const MAX_ITERATIONS: u32 = 1_000_000;

/// The finest clock resolution this crate is willing to believe, µs.
///
/// One nanosecond. A probe that reports finer than that — including the `0` a
/// frozen clock yields through [`Resolution::lower_bound_us`] — is clamped to
/// it. Clamping to `f64::MIN_POSITIVE` instead would let `achieved_ratio`
/// overflow to infinity for any real workload, which is worse than being
/// slightly pessimistic about a clock nobody has.
pub const MIN_RESOLUTION_US: f64 = 1e-3;

/// The result of sizing a workload against a clock.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Scaling {
    /// How many times to repeat the workload. Always at least 1 and at most
    /// [`MAX_ITERATIONS`].
    pub iterations: u32,

    /// The clock resolution the decision was based on, µs.
    pub resolution_us: f64,

    /// Cost of one iteration, µs.
    ///
    /// When [`Scaling::observed`] is `false` this is a **substitution**, not
    /// an observation: the batch did not register on the clock, so the only
    /// defensible per-op figure is "at most `resolution_us / count`".
    pub per_op_us: f64,

    /// `per_op_us * iterations`, µs: the phase duration this count buys.
    pub projected_phase_us: f64,

    /// `projected_phase_us / resolution_us`.
    pub achieved_ratio: f64,

    /// Whether `per_op_us` came from an observation rather than a substitution.
    ///
    /// A `false` here means the target is *provisionally* met at best: the
    /// projection rests on an upper bound, so the real ratio may be lower.
    ///
    /// Named for what `true` means, not for what `false` means: the field
    /// used to be called `estimated`, which read as "this number is a guess"
    /// while `true` actually meant the opposite -- that the batch registered
    /// on the clock and `per_op_us` is measured. It is internal to this
    /// module, but W3.3 puts it on screen, where a backwards name is a
    /// backwards claim.
    pub observed: bool,

    /// Whether `achieved_ratio` reached [`TARGET_RATIO`] **and** rests on a
    /// real measurement.
    pub target_met: bool,
}

impl Scaling {
    /// A single line for CLI output, including the verdict.
    #[must_use]
    pub fn describe(&self) -> String {
        let verdict = if self.target_met {
            "ok"
        } else if self.observed {
            "BELOW TARGET - treat as unresolved"
        } else {
            "UNMEASURABLE - below the clock, widen the workload"
        };
        format!(
            "{} iterations x {:.4} us/op = {:.1} ms ({:.0}x the {:.3} us clock) [{}]",
            self.iterations,
            self.per_op_us,
            self.projected_phase_us / 1_000.0,
            self.achieved_ratio,
            self.resolution_us,
            verdict,
        )
    }
}

/// Chooses an iteration count from a batch measurement.
///
/// `measured_us` is how long `measured_count` iterations took;
/// `resolution_us` is the measured clock resolution.
///
/// # The unmeasurable case
///
/// If the batch did not register (`measured_us <= 0`, or not finite), no per-op
/// figure can be observed. The substitution is `resolution_us / count`: the
/// batch took *at most* one clock tick, so one iteration costs at most that.
/// The resulting count is [`TARGET_RATIO`] × `count` — an escalation factor,
/// which is what lets the caller come back with a batch the clock can see.
#[must_use]
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the value is finite, at least 1.0, and clamped to MAX_ITERATIONS before the cast"
)]
pub fn scale_iterations(measured_us: f64, measured_count: u32, resolution_us: f64) -> Scaling {
    let resolution_us = sanitise(resolution_us);
    let count = f64::from(measured_count.max(1));

    let observed = measured_us.is_finite() && measured_us > 0.0;
    let per_op_us = if observed {
        measured_us / count
    } else {
        resolution_us / count
    };

    let needed = (TARGET_RATIO * resolution_us / per_op_us).ceil();
    let iterations = needed.min(f64::from(MAX_ITERATIONS)).max(1.0) as u32;

    let projected_phase_us = f64::from(iterations) * per_op_us;
    let achieved_ratio = projected_phase_us / resolution_us;

    Scaling {
        iterations,
        resolution_us,
        per_op_us,
        projected_phase_us,
        achieved_ratio,
        observed,
        target_met: observed && achieved_ratio + 1e-9 >= TARGET_RATIO,
    }
}

/// The outcome of [`plan_iterations`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Plan {
    /// The sizing decision.
    pub scaling: Scaling,

    /// How many times `workload` was invoked while planning.
    ///
    /// Planning is not free: these calls are in addition to the
    /// `scaling.iterations` the caller will run for the report.
    pub calls_spent: u32,
}

/// Measures `workload` and returns an iteration count it can be timed at.
///
/// Drives [`scale_iterations`] in rounds. The first round times a single call;
/// if that did not register on the clock, the next round times the batch the
/// previous round asked for, and so on. It stops when the batch is measurable
/// and the target is met, when the count stops growing, or at
/// [`MAX_ITERATIONS`].
///
/// The first call is a warm-up and is not timed: it pays for lazy
/// initialisation, first-touch page faults and, on `wasm32`, the tier-up from
/// the baseline compiler to the optimising one. Timing it would inflate the
/// estimate and therefore *shrink* the iteration count, which is the unsafe
/// direction.
///
/// The returned `scaling.target_met` may be `false`. Callers must surface that
/// rather than publishing the number regardless.
#[must_use]
pub fn plan_iterations(clock: &dyn Clock, mut workload: impl FnMut()) -> Plan {
    let resolution: Resolution = probe_resolution(clock, DEFAULT_PROBE_SAMPLES);
    let resolution_us = resolution.lower_bound_us();

    workload();
    let mut calls_spent = 1_u32;
    let mut count = 1_u32;

    loop {
        let started_us = clock.now_us();
        for _ in 0..count {
            workload();
        }
        let elapsed_us = clock.now_us() - started_us;
        calls_spent = calls_spent.saturating_add(count);

        let scaling = scale_iterations(elapsed_us, count, resolution_us);
        let done = scaling.target_met || scaling.iterations <= count || count >= MAX_ITERATIONS;
        if done {
            return Plan {
                scaling,
                calls_spent,
            };
        }
        count = scaling.iterations;
    }
}

/// Replaces a resolution with a usable, plausible one.
///
/// A resolution of 0, a negative one, or a non-finite one is not a clock
/// property; it is a broken probe. Those clamp to [`MIN_RESOLUTION_US`], which
/// is deliberately pessimistic — a nanosecond clock is finer than anything real,
/// so the clamp can only ever raise the iteration count.
fn sanitise(value: f64) -> f64 {
    if value.is_finite() && value > 0.0 {
        value.max(MIN_RESOLUTION_US)
    } else {
        MIN_RESOLUTION_US
    }
}

#[cfg(test)]
mod tests {
    use super::{
        plan_iterations, scale_iterations, Scaling, MAX_ITERATIONS, MIN_RESOLUTION_US, TARGET_RATIO,
    };
    use crate::clock::Clock;
    use std::cell::Cell;

    /// A clock whose every reading is `TICK_US` later than the last.
    ///
    /// Advancing on read rather than on demand is what a real clock does, and
    /// it is what lets the resolution probe see a real quantum.
    #[derive(Debug)]
    struct FakeClock {
        now_us: Cell<f64>,
    }

    impl FakeClock {
        const TICK_US: f64 = 10.0;

        fn new() -> Self {
            Self {
                now_us: Cell::new(0.0),
            }
        }
    }

    impl Clock for FakeClock {
        fn now_us(&self) -> f64 {
            let now = self.now_us.get();
            self.now_us.set(now + Self::TICK_US);
            now
        }

        fn cross_origin_isolated(&self) -> bool {
            false
        }

        fn name(&self) -> &'static str {
            "fake"
        }
    }

    #[test]
    fn a_phase_measuring_exactly_the_resolution_needs_target_ratio_iterations() {
        let scaling = scale_iterations(100.0, 1, 100.0);
        assert_eq!(scaling.iterations, 1_000);
        assert!(scaling.observed);
        assert!(scaling.target_met);
        assert!((scaling.achieved_ratio - TARGET_RATIO).abs() < 1e-6);
    }

    #[test]
    fn a_slower_phase_needs_fewer_iterations() {
        let scaling = scale_iterations(10_000.0, 1, 100.0);
        assert_eq!(scaling.iterations, 10);
        assert!(scaling.target_met);
        assert!((scaling.achieved_ratio - TARGET_RATIO).abs() < 1e-6);
    }

    #[test]
    fn a_batch_is_divided_by_its_count_not_treated_as_one_op() {
        // 100 iterations taking 3 ms is 30 us/op, not 3 ms/op.
        let scaling = scale_iterations(3_000.0, 100, 100.0);
        assert!((scaling.per_op_us - 30.0).abs() < f64::EPSILON);
        assert_eq!(scaling.iterations, 3_334);
        assert!(scaling.target_met);
    }

    #[test]
    fn counting_rounds_up_never_down() {
        // 100 iterations in 30 ms is 300 us/op. Against a 100 us clock that
        // needs 1000*100/300 = 333.33 iterations, which must round to 334:
        // 333 would land at 999x, just under the rule.
        let scaling = scale_iterations(30_000.0, 100, 100.0);
        assert!((scaling.per_op_us - 300.0).abs() < f64::EPSILON);
        assert_eq!(scaling.iterations, 334);
        assert!(scaling.achieved_ratio >= TARGET_RATIO);
        assert!((scaling.achieved_ratio - 1_002.0).abs() < 1e-9);
    }

    #[test]
    fn an_unmeasurable_batch_is_marked_as_not_observed() {
        let scaling = scale_iterations(0.0, 1, 100.0);
        // At most one tick for one op, so at most 100 us/op.
        assert!((scaling.per_op_us - 100.0).abs() < f64::EPSILON);
        assert!(!scaling.observed);
        assert!(!scaling.target_met);
        assert!(scaling.describe().contains("UNMEASURABLE"));
    }

    #[test]
    fn an_unmeasurable_batch_escalates_by_the_target_ratio() {
        // One tick spread over 1000 ops: 0.1 us/op at worst, so a million.
        let scaling = scale_iterations(0.0, 1_000, 100.0);
        assert!((scaling.per_op_us - 0.1).abs() < 1e-12);
        assert_eq!(scaling.iterations, MAX_ITERATIONS);
        assert!(!scaling.observed);
    }

    #[test]
    fn the_cap_is_reachable_and_is_reported_as_unmet() {
        // A million ops that still did not move the clock.
        let scaling = scale_iterations(0.0, MAX_ITERATIONS, 100.0);
        assert_eq!(scaling.iterations, MAX_ITERATIONS);
        assert!(!scaling.target_met);
        assert!(scaling.achieved_ratio < TARGET_RATIO);
    }

    #[test]
    fn a_phase_faster_than_the_cap_lands_on_the_cap_as_unresolved() {
        // 1e-9 us/op, genuinely measured over a batch of a million: hopeless.
        let scaling = scale_iterations(1e-3, MAX_ITERATIONS, 100.0);
        assert_eq!(scaling.iterations, MAX_ITERATIONS);
        assert!(scaling.observed);
        assert!(!scaling.target_met);
        assert!(scaling.describe().contains("BELOW TARGET"));
    }

    #[test]
    fn a_garbage_resolution_does_not_produce_a_garbage_count() {
        for bad in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            let scaling = scale_iterations(1_000.0, 1, bad);
            assert!(scaling.iterations >= 1);
            assert!(scaling.iterations <= MAX_ITERATIONS);
            assert!(
                (scaling.resolution_us - MIN_RESOLUTION_US).abs() < f64::EPSILON,
                "a broken probe must clamp to the plausible floor, got {}",
                scaling.resolution_us
            );
            assert!(scaling.achieved_ratio.is_finite());
        }
    }

    #[test]
    fn an_impossibly_fine_resolution_also_clamps() {
        let scaling = scale_iterations(1_000.0, 1, 1e-12);
        assert!((scaling.resolution_us - MIN_RESOLUTION_US).abs() < f64::EPSILON);
        assert!(scaling.achieved_ratio.is_finite());
    }

    #[test]
    fn a_garbage_measurement_does_not_produce_a_garbage_count() {
        for bad in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            let scaling = scale_iterations(bad, 1, 5.0);
            assert_eq!(scaling.iterations, 1_000);
            assert!(!scaling.observed);
        }
    }

    #[test]
    fn planning_registers_a_workload_whose_only_cost_is_the_clock() {
        let clock = FakeClock::new();
        // The workload does nothing; the two readings around one call are
        // themselves 10 us apart, so one call measures as 10 us.
        let plan = plan_iterations(&clock, || {});

        assert!((plan.scaling.resolution_us - FakeClock::TICK_US).abs() < f64::EPSILON);
        assert!(plan.scaling.observed);
        assert!(plan.scaling.target_met);
        // 10 us/op against a 10 us clock needs a thousand iterations.
        assert_eq!(plan.scaling.iterations, 1_000);
        // Warm-up plus the single-op first round.
        assert_eq!(plan.calls_spent, 2);
    }

    #[test]
    fn planning_reports_every_call_it_made() {
        let clock = FakeClock::new();
        let mut real_calls = 0_u32;
        let plan = plan_iterations(&clock, || real_calls += 1);

        assert_eq!(real_calls, plan.calls_spent);
        assert!(plan.calls_spent >= 2);
    }

    #[test]
    fn the_escalation_loop_terminates_in_a_few_rounds() {
        let mut count = 1_u32;
        let mut rounds = 0_u32;
        loop {
            let scaling = scale_iterations(0.0, count, 100.0);
            rounds += 1;
            let done = scaling.target_met || scaling.iterations <= count || count >= MAX_ITERATIONS;
            assert!(rounds < 10, "the escalation loop should terminate quickly");
            if done {
                break;
            }
            count = scaling.iterations;
        }
        assert!(rounds <= 3);
    }

    #[test]
    fn describe_reports_all_three_verdicts() {
        let met = Scaling {
            iterations: 1_000,
            resolution_us: 100.0,
            per_op_us: 100.0,
            projected_phase_us: 100_000.0,
            achieved_ratio: 1_000.0,
            observed: true,
            target_met: true,
        };
        assert!(met.describe().contains("[ok]"));

        let below = Scaling {
            iterations: MAX_ITERATIONS,
            resolution_us: 100.0,
            per_op_us: 1e-3,
            projected_phase_us: 1_000.0,
            achieved_ratio: 10.0,
            observed: true,
            target_met: false,
        };
        assert!(below.describe().contains("BELOW TARGET"));

        let unmeasurable = Scaling {
            observed: false,
            target_met: false,
            ..met
        };
        assert!(unmeasurable.describe().contains("UNMEASURABLE"));
    }
}
