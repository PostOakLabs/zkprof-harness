// SPDX-License-Identifier: Apache-2.0

//! Clock sources behind one trait, plus a *measured* timer-resolution probe.
//!
//! Two implementations exist:
//!
//! | target   | type                | source                       |
//! |----------|---------------------|------------------------------|
//! | native   | [`InstantClock`]    | `std::time::Instant`         |
//! | `wasm32` | [`PerformanceClock`] | `performance.now()`          |
//!
//! # Why the resolution is measured, not documented
//!
//! Browsers coarsen `performance.now()` to protect against timing side
//! channels. The coarsening depends on whether the page is cross-origin
//! isolated, and the isolation state is a property of the *deployment*, not of
//! the code. Any figure hard-coded here would be a guess about a deployment
//! this crate cannot see.
//!
//! [`probe_resolution`] therefore measures it: it takes consecutive readings
//! and reports the smallest nonzero delta. For a coarsened clock that delta is
//! the coarsening quantum itself.

use std::fmt;

/// Number of consecutive readings [`probe_resolution`] takes by default.
///
/// 10 000 is enough that a coarsened clock is guaranteed to step at least once
/// (a 100 µs clock steps every reading, a 5 µs clock likewise) while staying
/// cheap enough to run once per process.
pub const DEFAULT_PROBE_SAMPLES: u32 = 10_000;

/// A monotonic time source.
///
/// Implementations must never go backwards. The origin is arbitrary: only
/// differences between two readings are meaningful.
///
/// `Debug` is a supertrait so that [`crate::Profiler`] can derive it. The
/// derived output names the clock, which is what makes a debug dump of a
/// profiler say which clock a measurement came from.
pub trait Clock: fmt::Debug {
    /// Current reading in microseconds.
    fn now_us(&self) -> f64;

    /// Value of the platform's `crossOriginIsolated` flag.
    ///
    /// Off the web platform the flag does not exist, so this is always
    /// `false`. That is not a claim that the clock is coarsened — native clocks
    /// are not — just that the isolation question does not apply.
    fn cross_origin_isolated(&self) -> bool;

    /// Short identifier for the source, for log lines and report footnotes.
    fn name(&self) -> &'static str;
}

/// Outcome of a clock-resolution probe.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Resolution {
    /// The probe saw the clock step, and this is the smallest step it saw.
    Measured {
        /// Smallest nonzero delta between two consecutive readings, µs.
        min_nonzero_delta_us: f64,
        /// How many readings the probe took.
        samples: u32,
    },
    /// The clock never advanced across `samples` consecutive readings.
    ///
    /// This is a stopped or pathologically coarse clock. It is reported rather
    /// than silently substituted, because "the clock did not move" and "the
    /// clock is infinitely fine" are very different situations that would
    /// otherwise serialize to the same number.
    NoAdvance {
        /// How many readings the probe took.
        samples: u32,
        /// Wall time the probe itself spanned, µs. The true resolution is
        /// strictly worse than this.
        probe_span_us: f64,
    },
}

impl Resolution {
    /// A conservative resolution figure in microseconds, safe for sizing.
    ///
    /// For [`Resolution::Measured`] this is the probe result. For
    /// [`Resolution::NoAdvance`] it is the probe's own wall-time span: the
    /// clock is known to be *coarser* than that, so using it sizes iteration
    /// counts **up**, never down. Guessing a smaller number here would
    /// under-scale the workload and produce confident-looking noise.
    #[must_use]
    pub fn lower_bound_us(&self) -> f64 {
        match *self {
            Self::Measured {
                min_nonzero_delta_us,
                ..
            } => min_nonzero_delta_us,
            Self::NoAdvance { probe_span_us, .. } => probe_span_us,
        }
    }

    /// True when the probe actually observed the clock step at least once.
    #[must_use]
    pub fn observed(&self) -> bool {
        matches!(self, Self::Measured { .. })
    }

    /// How many readings the probe took.
    #[must_use]
    pub fn samples(&self) -> u32 {
        match *self {
            Self::Measured { samples, .. } | Self::NoAdvance { samples, .. } => samples,
        }
    }
}

impl fmt::Display for Resolution {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::Measured {
                min_nonzero_delta_us,
                samples,
            } => write!(
                f,
                "{min_nonzero_delta_us:.3} us (min nonzero delta over {samples} readings)"
            ),
            Self::NoAdvance {
                samples,
                probe_span_us,
            } => write!(
                f,
                "coarser than {probe_span_us:.3} us (clock did not advance in {samples} readings)"
            ),
        }
    }
}

/// Measures the smallest nonzero delta observable on `clock`.
///
/// The probe takes `samples + 1` readings and keeps the smallest strictly
/// positive difference between consecutive ones. On a clock coarsened to a
/// quantum `q`, consecutive readings are either identical or differ by a
/// multiple of `q`, so the result is `q`.
#[must_use]
pub fn probe_resolution(clock: &dyn Clock, samples: u32) -> Resolution {
    let started_us = clock.now_us();
    let mut previous_us = started_us;
    let mut best_us = f64::INFINITY;

    for _ in 0..samples {
        let now_us = clock.now_us();
        let delta_us = now_us - previous_us;
        if delta_us > 0.0 && delta_us < best_us {
            best_us = delta_us;
        }
        previous_us = now_us;
    }

    let ended_us = clock.now_us();

    if best_us.is_finite() {
        Resolution::Measured {
            min_nonzero_delta_us: best_us,
            samples,
        }
    } else {
        Resolution::NoAdvance {
            samples,
            probe_span_us: (ended_us - started_us).max(0.0),
        }
    }
}

/// Why a clock could not be constructed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClockError {
    /// The global object (`globalThis`) was unavailable.
    NoGlobalScope,
    /// `performance` was not reachable on the global object.
    NoPerformance,
}

impl fmt::Display for ClockError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoGlobalScope => f.write_str("no global object"),
            Self::NoPerformance => f.write_str("`performance` is unreachable on the global object"),
        }
    }
}

impl std::error::Error for ClockError {}

/// Constructs the clock appropriate for the current target.
///
/// # Errors
///
/// Returns [`ClockError`] on `wasm32` when `window.performance` cannot be
/// reached. Never fails on native targets.
#[cfg(not(target_arch = "wasm32"))]
#[allow(
    clippy::unnecessary_wraps,
    reason = "both platforms must expose the same signature"
)]
pub fn platform_clock() -> Result<Box<dyn Clock>, ClockError> {
    Ok(Box::new(InstantClock::new()))
}

/// Constructs the clock appropriate for the current target.
///
/// # Errors
///
/// Returns [`ClockError`] when `performance` cannot be reached on the global
/// object. On `wasm32` that happens outside any JavaScript realm; it does
/// **not** happen in a Web Worker, because [`PerformanceClock`] resolves
/// `performance` from `globalThis` rather than from `window`.
#[cfg(target_arch = "wasm32")]
pub fn platform_clock() -> Result<Box<dyn Clock>, ClockError> {
    Ok(Box::new(PerformanceClock::new()?))
}

/// Clock backed by [`std::time::Instant`].
#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug, Clone)]
pub struct InstantClock {
    origin: std::time::Instant,
}

#[cfg(not(target_arch = "wasm32"))]
impl InstantClock {
    /// Creates a clock whose zero point is now.
    #[must_use]
    pub fn new() -> Self {
        Self {
            origin: std::time::Instant::now(),
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Default for InstantClock {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Clock for InstantClock {
    fn now_us(&self) -> f64 {
        // `as_secs_f64` keeps sub-nanosecond precision for any elapsed time a
        // profiling run can plausibly reach, so no separate nanos path.
        self.origin.elapsed().as_secs_f64() * 1_000_000.0
    }

    fn cross_origin_isolated(&self) -> bool {
        false
    }

    fn name(&self) -> &'static str {
        "std::time::Instant"
    }
}

/// Clock backed by `performance.now()`.
///
/// # Where it works
///
/// `performance` is resolved from `globalThis`, not from `window`. That
/// distinction is the whole reason this type exists in two places: a
/// `window`-based clock cannot be constructed inside a Web Worker, and the
/// browser harness runs its workload in one so the page stays responsive.
/// `globalThis.performance` is present in windows, dedicated workers and
/// shared workers alike.
///
/// `crossOriginIsolated` is likewise read from `globalThis`. A dedicated
/// worker inherits its owning page's isolation state, so the figure measured
/// in a worker is the figure that describes the page.
#[cfg(target_arch = "wasm32")]
#[derive(Clone)]
pub struct PerformanceClock {
    performance: web_sys::Performance,
}

#[cfg(target_arch = "wasm32")]
impl fmt::Debug for PerformanceClock {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PerformanceClock")
            .field("name", &self.name())
            .field("cross_origin_isolated", &self.cross_origin_isolated())
            .finish_non_exhaustive()
    }
}

#[cfg(target_arch = "wasm32")]
impl PerformanceClock {
    /// Captures the global `performance` object.
    ///
    /// # Errors
    ///
    /// Returns [`ClockError::NoPerformance`] when `globalThis.performance` is
    /// missing or is not a `Performance`.
    pub fn new() -> Result<Self, ClockError> {
        let performance = js_sys::Reflect::get(
            &js_sys::global(),
            &wasm_bindgen::JsValue::from_str("performance"),
        )
        .map_err(|_| ClockError::NoPerformance)?;
        let performance = wasm_bindgen::JsCast::dyn_into::<web_sys::Performance>(performance)
            .map_err(|_| ClockError::NoPerformance)?;
        Ok(Self { performance })
    }
}

#[cfg(target_arch = "wasm32")]
impl Clock for PerformanceClock {
    fn now_us(&self) -> f64 {
        // `performance.now()` is milliseconds since the time origin, as f64.
        // Scaling to microseconds keeps ~8 decimal digits of headroom at
        // typical page ages, far finer than any browser quantum.
        self.performance.now() * 1_000.0
    }

    fn cross_origin_isolated(&self) -> bool {
        // web-sys 0.3.104 ships no generated binding for `crossOriginIsolated`
        // -- grepping the crate's generated sources finds no mention of it --
        // so read it reflectively off the global object. A missing flag or a
        // non-boolean value means "not isolated", which is the safe reading:
        // it implies a coarsened clock and therefore more iterations, never
        // fewer.
        js_sys::Reflect::get(
            &js_sys::global(),
            &wasm_bindgen::JsValue::from_str("crossOriginIsolated"),
        )
        .ok()
        .and_then(|value| value.as_bool())
        .unwrap_or(false)
    }

    fn name(&self) -> &'static str {
        "performance.now()"
    }
}

#[cfg(test)]
mod tests {
    use super::{probe_resolution, Clock, Resolution, DEFAULT_PROBE_SAMPLES};
    use std::cell::Cell;

    /// A clock that never moves. Stands in for a stopped or pathologically
    /// coarse source.
    #[derive(Debug)]
    struct FrozenClock {
        at_us: f64,
    }

    impl Clock for FrozenClock {
        fn now_us(&self) -> f64 {
            self.at_us
        }

        fn cross_origin_isolated(&self) -> bool {
            false
        }

        fn name(&self) -> &'static str {
            "frozen"
        }
    }

    /// A clock that only ever reports whole quanta, the way a coarsened browser
    /// clock does. It wraps every four ticks, so a probe that kept negative
    /// deltas would report a negative resolution.
    #[derive(Debug)]
    struct CoarsenedClock {
        ticks: Cell<u64>,
        quantum_us: f64,
    }

    impl Clock for CoarsenedClock {
        fn now_us(&self) -> f64 {
            let tick = self.ticks.get();
            self.ticks.set(tick + 1);
            f64::from(u32::try_from(tick % 4).unwrap_or(0)) * self.quantum_us
        }

        fn cross_origin_isolated(&self) -> bool {
            false
        }

        fn name(&self) -> &'static str {
            "coarsened"
        }
    }

    #[test]
    fn a_coarsened_clock_reports_its_quantum() {
        let clock = CoarsenedClock {
            ticks: Cell::new(0),
            quantum_us: 100.0,
        };

        assert_eq!(
            probe_resolution(&clock, 1_000),
            Resolution::Measured {
                min_nonzero_delta_us: 100.0,
                samples: 1_000,
            }
        );
    }

    #[test]
    fn the_probe_ignores_a_backwards_step_rather_than_reporting_it() {
        // Starting at tick 3 means the very next reading wraps to 0, a delta
        // of -300 us. A monotonic clock cannot step backwards, so a negative
        // delta here means the model clock wrapped; the probe must skip it.
        let clock = CoarsenedClock {
            ticks: Cell::new(3),
            quantum_us: 100.0,
        };

        assert_eq!(
            probe_resolution(&clock, 500),
            Resolution::Measured {
                min_nonzero_delta_us: 100.0,
                samples: 500,
            }
        );
    }

    #[test]
    fn a_clock_that_never_advances_is_reported_not_guessed() {
        let clock = FrozenClock { at_us: 7.0 };

        assert_eq!(
            probe_resolution(&clock, 50),
            Resolution::NoAdvance {
                samples: 50,
                probe_span_us: 0.0,
            }
        );
    }

    #[test]
    fn no_advance_still_yields_a_usable_lower_bound() {
        let resolution = probe_resolution(&FrozenClock { at_us: 7.0 }, 10);

        assert!(!resolution.observed());
        assert!(resolution.lower_bound_us() <= 0.0);
        assert_eq!(resolution.samples(), 10);
        assert!(resolution.to_string().contains("coarser than"));
    }

    #[test]
    fn measured_resolution_carries_its_own_lower_bound() {
        let resolution = Resolution::Measured {
            min_nonzero_delta_us: 5.0,
            samples: DEFAULT_PROBE_SAMPLES,
        };

        assert!(resolution.observed());
        assert!((resolution.lower_bound_us() - 5.0).abs() < f64::EPSILON);
        assert!(resolution.to_string().starts_with("5.000 us"));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn the_native_clock_probes_to_a_finite_positive_resolution() {
        let clock = super::InstantClock::new();
        let resolution = probe_resolution(&clock, DEFAULT_PROBE_SAMPLES);

        assert!(
            resolution.observed(),
            "the native clock should advance: {resolution}"
        );
        assert!(resolution.lower_bound_us() > 0.0);
        // No sane platform clock is coarser than a millisecond.
        assert!(
            resolution.lower_bound_us() < 1_000.0,
            "implausibly coarse: {resolution}"
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn the_native_clock_reports_no_cross_origin_isolation() {
        let clock = super::InstantClock::new();
        // The flag does not exist off the web platform. Reporting false is a
        // statement about the platform, not about the clock's precision.
        assert!(!clock.cross_origin_isolated());
        assert_eq!(clock.name(), "std::time::Instant");
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn the_platform_clock_is_the_native_clock_here() {
        let clock = super::platform_clock().expect("native clocks do not fail");
        assert_eq!(clock.name(), "std::time::Instant");
    }
}
