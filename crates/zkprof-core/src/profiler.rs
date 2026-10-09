// SPDX-License-Identifier: Apache-2.0

//! The span API: accumulate wall time per label, then emit a [`Report`].
//!
//! # Usage
//!
//! ```no_run
//! use zkprof_core::clock::platform_clock;
//! use zkprof_core::model::{Primitive, System, Target};
//! use zkprof_core::profiler::Profiler;
//!
//! let clock = platform_clock().unwrap();
//! let profiler = Profiler::new(clock.as_ref(), Target::native(), System::new("groth16", "bn254"), "demo");
//!
//! profiler.span(Primitive::Msm, || { /* … */ });
//! profiler.span("witness", || { /* … */ });
//!
//! let report = profiler.finish().unwrap();
//! ```
//!
//! # Why `&self` and not `&mut self`
//!
//! Spans nest. With `&mut self`, an inner span could not borrow the profiler
//! while an outer span was open, so nesting would not compile. The
//! accumulators therefore live behind a [`RefCell`] and every method takes
//! `&self`. The borrow is held only for the duration of a bookkeeping update,
//! never across the measured body.
//!
//! # What is *not* measured
//!
//! Time inside the measured region but outside any span is not attributed to
//! any primitive. It still counts towards `total_ms`, so
//! [`Report::unattributed_ms`] tells you how much of the run you failed to
//! instrument.

use std::cell::RefCell;
use std::collections::BTreeMap;

use crate::clock::{probe_resolution, Clock, Resolution, DEFAULT_PROBE_SAMPLES};
use crate::model::{
    Memory, Phase, Primitive, PrimitiveRow, Report, ReportError, System, Target, Threads,
};
use crate::schema::{REPORT_VERSION, SCHEMA_ID};

/// Where a span's elapsed time is accumulated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Label {
    /// Accumulates into the report's `primitives` array.
    Primitive(Primitive),
    /// Accumulates into the report's `phases` array, under this name.
    ///
    /// Phase names are free-form. A string that is not one of the seven
    /// [`Primitive`] spellings becomes a phase, which is why [`Label::from`]
    /// never fails.
    Phase(String),
}

impl From<Primitive> for Label {
    fn from(primitive: Primitive) -> Self {
        Self::Primitive(primitive)
    }
}

impl From<&str> for Label {
    fn from(name: &str) -> Self {
        Self::Phase(name.to_owned())
    }
}

impl From<String> for Label {
    fn from(name: String) -> Self {
        Self::Phase(name)
    }
}

impl std::fmt::Display for Label {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Primitive(primitive) => f.write_str(primitive.as_str()),
            Self::Phase(name) => f.write_str(name),
        }
    }
}

/// Accumulates wall time per label and builds one report.
///
/// The measured region opens when the profiler is constructed and closes when
/// [`Profiler::finish`] is called. Anything done between construction and the
/// first span — loading a circuit, allocating buffers — is counted in
/// `total_ms` but attributed to no primitive.
#[derive(Debug)]
pub struct Profiler<'c> {
    clock: &'c dyn Clock,
    region_started_us: f64,
    inner: RefCell<Inner>,
}

#[derive(Debug)]
struct Inner {
    target: Target,
    system: System,
    workload_id: String,
    memory: Memory,
    threads: Threads,
    iterations: u32,
    resolution: Resolution,
    run_manifest_ref: Option<String>,
    /// Set by [`Profiler::set_region_ms`]. `None` means "time it myself".
    region_ms_override: Option<f64>,
    primitive_us: BTreeMap<Primitive, f64>,
    primitive_order: Vec<Primitive>,
    phase_us: Vec<(String, f64)>,
    open: Vec<Frame>,
    backend_id: Option<String>,
    version: Option<String>,
    commitment_scheme: Option<String>,
}

#[derive(Debug)]
struct Frame {
    label: Label,
    started_us: f64,
}

impl Inner {
    fn record(&mut self, label: Label, elapsed_us: f64) {
        match label {
            Label::Primitive(primitive) => {
                if !self.primitive_order.contains(&primitive) {
                    self.primitive_order.push(primitive);
                }
                *self.primitive_us.entry(primitive).or_insert(0.0) += elapsed_us;
            }
            Label::Phase(name) => match self
                .phase_us
                .iter_mut()
                .find(|(existing, _)| *existing == name)
            {
                Some((_, accumulated_us)) => *accumulated_us += elapsed_us,
                None => self.phase_us.push((name, elapsed_us)),
            },
        }
    }
}

impl<'c> Profiler<'c> {
    /// Opens the measured region.
    ///\n    /// The clock's resolution is probed here, once, and cached: probing costs
    /// [`DEFAULT_PROBE_SAMPLES`] readings and the answer does not change during
    /// a run.
    #[must_use]
    pub fn new(
        clock: &'c dyn Clock,
        target: Target,
        system: System,
        workload_id: impl Into<String>,
    ) -> Self {
        let resolution = probe_resolution(clock, DEFAULT_PROBE_SAMPLES);
        let region_started_us = clock.now_us();
        Self {
            clock,
            region_started_us,
            inner: RefCell::new(Inner {
                target,
                system,
                workload_id: workload_id.into(),
                memory: Memory::unmeasured(),
                threads: Threads::single(),
                iterations: 1,
                resolution,
                run_manifest_ref: None,
                region_ms_override: None,
                primitive_us: BTreeMap::new(),
                primitive_order: Vec::new(),
                phase_us: Vec::new(),
                open: Vec::new(),
                backend_id: None,
                version: None,
                commitment_scheme: None,
            }),
        }
    }

    /// Sets the iteration count. Defaults to 1.
    #[must_use]
    pub fn with_iterations(mut self, iterations: u32) -> Self {
        self.inner.get_mut().iterations = iterations;
        self
    }

    /// Sets the threading description. Defaults to single-threaded.
    #[must_use]
    pub fn with_threads(mut self, threads: Threads) -> Self {
        self.inner.get_mut().threads = threads;
        self
    }

    /// Sets the heap measurement. Defaults to unmeasured.
    #[must_use]
    pub fn with_memory(mut self, memory: Memory) -> Self {
        self.inner.get_mut().memory = memory;
        self
    }

    /// Records a memory measurement taken *after* the workload ran.
    ///
    /// [`Profiler::with_memory`] runs at construction, before there is anything
    /// to measure. A high-water mark is by definition observable only at the
    /// end, so this is the setter a real probe needs. Calling it twice keeps
    /// the last value, which is the same rule the builder setters follow.
    pub fn record_memory(&self, memory: Memory) {
        self.inner.borrow_mut().memory = memory;
    }

    /// Overrides the probed clock resolution.
    ///
    /// Intended for tests and for hosts that have measured the resolution more
    /// carefully than a 10 000-reading probe can.
    #[must_use]
    pub fn with_resolution(mut self, resolution: Resolution) -> Self {
        self.inner.get_mut().resolution = resolution;
        self
    }

    /// Attaches an `edi-schema` run-manifest reference.
    #[must_use]
    pub fn with_run_manifest_ref(mut self, reference: impl Into<String>) -> Self {
        self.inner.get_mut().run_manifest_ref = Some(reference.into());
        self
    }

    /// Sets the proving backend identity.
    #[must_use]
    pub fn with_backend_id(mut self, backend_id: impl Into<String>) -> Self {
        self.inner.get_mut().backend_id = Some(backend_id.into());
        self
    }

    /// Sets the backend implementation version.
    #[must_use]
    pub fn with_version(mut self, version: impl Into<String>) -> Self {
        self.inner.get_mut().version = Some(version.into());
        self
    }

    /// Sets the polynomial commitment scheme.
    #[must_use]
    pub fn with_commitment_scheme(mut self, commitment_scheme: impl Into<String>) -> Self {
        self.inner.get_mut().commitment_scheme = Some(commitment_scheme.into());
        self
    }

    /// Records a duration that was measured somewhere other than a span.
    ///
    /// A [`Profiler`] normally attributes only time it timed itself. Some
    /// regions cannot be timed that way, because part of them is not in this
    /// process. The JS↔WASM boundary is the case that motivated this method:
    /// a benchmark that crosses it once per iteration spends part of every
    /// iteration in JavaScript, and no Rust closure encloses that part. The
    /// host times the loop and hands the remainder in.
    ///
    /// Durations are microseconds and accumulate onto whatever is already in
    /// `label`, exactly as a span's duration would. Non-finite and non-positive
    /// values are dropped rather than clamped: a negative elapsed time is a
    /// broken measurement, and subtracting one from a total would *reduce* the
    /// time attributed to work that demonstrably happened.
    ///
    /// # What this does not do
    ///
    /// It does not check that the duration is plausible, and it cannot — the
    /// caller owns the clock. The report carries the figure as measured.
    pub fn record(&self, label: impl Into<Label>, elapsed_us: f64) {
        if !elapsed_us.is_finite() || elapsed_us <= 0.0 {
            return;
        }
        self.inner.borrow_mut().record(label.into(), elapsed_us);
    }

    /// Replaces the measured region's duration with a host-supplied one.
    ///
    /// [`Profiler::finish`] normally derives `total_ms` from this profiler's
    /// own clock, which sees only what ran inside this process. When part of
    /// the region ran outside it — the JS↔WASM boundary again — the host is
    /// the only party that can see the whole region, so the host's figure is
    /// the one the report must carry.
    ///
    /// Call this before [`Profiler::finish`]. The value is milliseconds and is
    /// clamped at zero for the same reason [`Profiler::record`] drops
    /// negatives: a negative total is a broken measurement, not a fast
    /// workload.
    pub fn set_region_ms(&self, total_ms: f64) {
        let sanitised = if total_ms.is_finite() {
            total_ms.max(0.0)
        } else {
            0.0
        };
        self.inner.borrow_mut().region_ms_override = Some(sanitised);
    }

    /// The clock resolution this profiler is working against.
    #[must_use]
    pub fn resolution(&self) -> Resolution {
        self.inner.borrow().resolution
    }

    /// Wall time of the measured region so far, milliseconds.
    #[must_use]
    pub fn elapsed_ms(&self) -> f64 {
        ((self.clock.now_us() - self.region_started_us).max(0.0)) / 1_000.0
    }

    /// Opens a span, returning a guard that closes it when dropped.
    ///
    /// Guards must nest LIFO. The guard closes whichever span is on top of the
    /// stack, so dropping two guards out of order attributes the durations to
    /// the wrong labels.
    #[must_use]
    pub fn start(&self, label: impl Into<Label>) -> SpanGuard<'_, 'c> {
        let started_us = self.clock.now_us();
        self.inner.borrow_mut().open.push(Frame {
            label: label.into(),
            started_us,
        });
        SpanGuard { profiler: self }
    }

    /// Times `body`, accumulating into `label`.
    ///
    /// The body's duration is recorded even if it panics: the guard's `Drop`
    /// runs during unwinding.
    ///
    /// The return value is the body's. It is deliberately not `#[must_use]`:
    /// the usual shape is `profiler.span(label, || { ... });` with a unit body,
    /// and requiring `let _ =` there would be noise.
    pub fn span<T>(&self, label: impl Into<Label>, body: impl FnOnce() -> T) -> T {
        let guard = self.start(label);
        let output = body();
        drop(guard);
        output
    }

    /// Microseconds accumulated in `primitive` so far.
    #[must_use]
    pub fn primitive_us(&self, primitive: Primitive) -> f64 {
        self.inner
            .borrow()
            .primitive_us
            .get(&primitive)
            .copied()
            .unwrap_or(0.0)
    }

    /// Closes the measured region and builds the report.
    ///
    /// # Errors
    ///
    /// `total_ms` is the profiler's own region unless the host overrode it with
    /// [`Profiler::set_region_ms`], which a boundary benchmark does because the
    /// region it is reporting on is partly in the host.
    ///
    /// # Errors
    ///
    /// - [`ReportError::SpanStillOpen`] if a guard outlived this call.
    /// - [`ReportError::NoMeasuredTime`] if no primitive span recorded any
    ///   elapsed time. Primitive shares are taken over the sum of primitive
    ///   time, so with nothing measured there is nothing to divide by.
    /// - Any other [`ReportError`] the assembled report fails validation on.
    pub fn finish(&self) -> Result<Report, ReportError> {
        let ended_us = self.clock.now_us();
        let inner = self.inner.borrow();

        if let Some(frame) = inner.open.last() {
            return Err(ReportError::SpanStillOpen {
                name: frame.label.to_string(),
            });
        }

        // A host-supplied region wins over the locally measured one. When it
        // is set, `region_started_us` still describes a real interval — the
        // profiler's own construction-to-finish span — but that interval is a
        // subset of the region the host timed, so using it would understate
        // the total.
        let region_us = (ended_us - self.region_started_us).max(0.0);
        let total_ms = inner.region_ms_override.unwrap_or(region_us / 1_000.0);

        let attributed_us: f64 = inner.primitive_us.values().sum();
        if attributed_us <= 0.0 {
            return Err(ReportError::NoMeasuredTime);
        }

        let primitives: Vec<PrimitiveRow> = inner
            .primitive_order
            .iter()
            .map(|primitive| {
                let us = inner.primitive_us.get(primitive).copied().unwrap_or(0.0);
                PrimitiveRow {
                    name: *primitive,
                    ms: us / 1_000.0,
                    share: us / attributed_us,
                }
            })
            .collect();

        let phases: Vec<Phase> = inner
            .phase_us
            .iter()
            .map(|(name, us)| {
                let ms = us / 1_000.0;
                Phase {
                    name: name.clone(),
                    ms,
                    share: if total_ms > 0.0 {
                        (ms / total_ms).clamp(0.0, 1.0)
                    } else {
                        0.0
                    },
                }
            })
            .collect();

        let report = Report {
            schema: Some(SCHEMA_ID.to_owned()),
            report_version: REPORT_VERSION.to_owned(),
            target: inner.target.clone(),
            system: inner.system.clone(),
            workload_id: inner.workload_id.clone(),
            total_ms,
            phases,
            primitives,
            memory: inner.memory,
            threads: inner.threads,
            cross_origin_isolated: self.clock.cross_origin_isolated(),
            timer_resolution_us: inner.resolution.lower_bound_us(),
            iterations: inner.iterations,
            run_manifest_ref: inner.run_manifest_ref.clone(),
            gpu_timing_source: None,
            backend_id: inner.backend_id.clone(),
            version: inner.version.clone(),
            commitment_scheme: inner.commitment_scheme.clone(),
        };

        report.validate()?;
        Ok(report)
    }
}

/// An open span. The duration is recorded when this is dropped.
#[derive(Debug)]
pub struct SpanGuard<'p, 'c> {
    profiler: &'p Profiler<'c>,
}

impl Drop for SpanGuard<'_, '_> {
    fn drop(&mut self) {
        let ended_us = self.profiler.clock.now_us();
        // Take the timestamp before touching the RefCell: `now_us` can be
        // arbitrarily slow and its cost must not land inside the span.
        let mut inner = self.profiler.inner.borrow_mut();
        if let Some(frame) = inner.open.pop() {
            let elapsed_us = (ended_us - frame.started_us).max(0.0);
            inner.record(frame.label, elapsed_us);
        }
    }
}

/// Opens a span for the duration of a block.
///
/// `span!(profiler, Primitive::Msm, { … })` accumulates into the `msm` row;
/// `span!(profiler, "setup", { … })` accumulates into a phase named `setup`.
/// The block's value is the expression's value.
///
/// Prefer [`Profiler::span`] in library code; this macro exists because the
/// closure form is awkward when the body contains `?` or `return`.
#[macro_export]
macro_rules! span {
    ($profiler:expr, $label:expr, $body:block) => {
        $profiler.span($label, || $body)
    };
}

#[cfg(test)]
mod tests {
    use super::{Label, Profiler};
    use crate::clock::{Clock, Resolution};
    use crate::model::{Memory, Primitive, ReportError, System, Target, Threads};
    use std::cell::Cell;

    /// A clock that only moves when the test advances it, so every number in
    /// these tests is exact rather than timing-dependent.
    #[derive(Debug)]
    struct ScriptedClock {
        now_us: Cell<f64>,
        isolated: bool,
    }

    impl ScriptedClock {
        fn new() -> Self {
            Self {
                now_us: Cell::new(1_000.0),
                isolated: false,
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
            self.isolated
        }
        fn name(&self) -> &'static str {
            "scripted"
        }
    }

    const RESOLUTION: Resolution = Resolution::Measured {
        min_nonzero_delta_us: 0.5,
        samples: 10_000,
    };

    fn profiler(clock: &ScriptedClock) -> Profiler<'_> {
        Profiler::new(
            clock,
            Target::native(),
            System::new("groth16", "bn254"),
            "demo",
        )
        .with_resolution(RESOLUTION)
    }

    fn tick(profiler: &Profiler<'_>, clock: &ScriptedClock, label: impl Into<Label>, us: f64) {
        profiler.span(label, || clock.advance(us));
    }

    #[test]
    fn time_is_attributed_to_the_named_primitive() {
        let clock = ScriptedClock::new();
        let profiler = profiler(&clock);

        tick(&profiler, &clock, Primitive::Msm, 7_000.0);
        tick(&profiler, &clock, Primitive::NttFft, 3_000.0);

        assert!((profiler.primitive_us(Primitive::Msm) - 7_000.0).abs() < f64::EPSILON);
        assert!((profiler.primitive_us(Primitive::NttFft) - 3_000.0).abs() < f64::EPSILON);
        assert!((profiler.primitive_us(Primitive::Hash) - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn repeated_spans_accumulate() {
        let clock = ScriptedClock::new();
        let profiler = profiler(&clock);

        for _ in 0..4 {
            tick(&profiler, &clock, Primitive::FieldArith, 250.0);
        }

        assert!((profiler.primitive_us(Primitive::FieldArith) - 1_000.0).abs() < f64::EPSILON);
    }

    /// The marshalling-bench shape: the host timed 10 ms of loop, of which the
    /// profiler itself only saw 2 ms, and 8 ms of it was the boundary.
    #[test]
    fn a_host_recorded_duration_accumulates_exactly_like_a_span() {
        let clock = ScriptedClock::new();
        let profiler = profiler(&clock);

        tick(&profiler, &clock, Primitive::Hash, 2_000.0);
        profiler.record(Primitive::Marshalling, 8_000.0);

        assert!((profiler.primitive_us(Primitive::Hash) - 2_000.0).abs() < f64::EPSILON);
        assert!((profiler.primitive_us(Primitive::Marshalling) - 8_000.0).abs() < f64::EPSILON);

        // Two calls to the same label add, as two spans would.
        profiler.record(Primitive::Marshalling, 1_000.0);
        assert!((profiler.primitive_us(Primitive::Marshalling) - 9_000.0).abs() < f64::EPSILON);
    }

    #[test]
    fn a_recorded_duration_splits_shares_the_same_way_a_span_does() {
        let clock = ScriptedClock::new();
        let profiler = profiler(&clock);
        profiler.set_region_ms(10.0);

        tick(&profiler, &clock, Primitive::Hash, 2_000.0);
        profiler.record(Primitive::Marshalling, 8_000.0);

        let report = profiler.finish().expect("report should build");
        assert!((report.total_ms - 10.0).abs() < 1e-9);
        assert!((report.primitive(Primitive::Hash).unwrap().share - 0.2).abs() < 1e-9);
        assert!((report.primitive(Primitive::Marshalling).unwrap().share - 0.8).abs() < 1e-9);
        // Attributed time is now the whole region, so nothing is unattributed.
        assert!(report.unattributed_ms().abs() < 1e-9);
    }

    #[test]
    fn a_host_supplied_region_replaces_the_locally_measured_one() {
        let clock = ScriptedClock::new();
        let profiler = profiler(&clock);

        tick(&profiler, &clock, Primitive::Msm, 1_000.0);
        // The profiler's own region is 1 ms; the host saw 25 ms of it.
        profiler.set_region_ms(25.0);

        let report = profiler.finish().expect("report should build");
        assert!((report.total_ms - 25.0).abs() < 1e-9);
    }

    #[test]
    fn a_host_supplied_region_is_clamped_at_zero_not_negated() {
        let clock = ScriptedClock::new();
        let profiler = profiler(&clock);

        tick(&profiler, &clock, Primitive::Msm, 1_000.0);
        profiler.set_region_ms(-5.0);

        let report = profiler.finish().expect("report should build");
        assert!((report.total_ms - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn a_non_finite_host_region_does_not_reach_the_report() {
        let clock = ScriptedClock::new();
        let profiler = profiler(&clock);

        tick(&profiler, &clock, Primitive::Msm, 1_000.0);
        profiler.set_region_ms(f64::NAN);

        let report = profiler.finish().expect("report should build");
        assert!(report.total_ms.is_finite());
        assert!((report.total_ms - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn a_negative_or_broken_recorded_duration_is_dropped_not_subtracted() {
        let clock = ScriptedClock::new();
        let profiler = profiler(&clock);

        tick(&profiler, &clock, Primitive::Msm, 1_000.0);
        profiler.record(Primitive::Marshalling, -3_000.0);
        profiler.record(Primitive::Marshalling, f64::NAN);
        profiler.record(Primitive::Marshalling, 0.0);

        // Dropped, so the msm row keeps the whole attribution rather than
        // being credited 3 ms of work that never happened.
        assert!((profiler.primitive_us(Primitive::Marshalling) - 0.0).abs() < f64::EPSILON);
        let report = profiler.finish().expect("report should build");
        assert_eq!(report.primitives.len(), 1);
    }

    #[test]
    fn a_run_built_entirely_from_a_host_duration_is_a_valid_report() {
        let clock = ScriptedClock::new();
        let profiler = profiler(&clock);

        // No span at all: the copy-baseline shape attributes the whole loop to
        // marshalling and times nothing inside wasm.
        profiler.set_region_ms(4.0);
        profiler.record(Primitive::Marshalling, 4_000.0);

        let report = profiler.finish().expect("report should build");
        assert!((report.total_ms - 4.0).abs() < 1e-9);
        assert_eq!(report.primitives.len(), 1);
        assert!((report.primitives[0].share - 1.0).abs() < 1e-9);
        assert!(report.phases.is_empty());
    }

    #[test]
    fn phases_use_free_form_names_and_do_not_touch_primitives() {
        let clock = ScriptedClock::new();
        let profiler = profiler(&clock);

        tick(&profiler, &clock, "setup", 1_000.0);
        tick(&profiler, &clock, "prove", 5_000.0);
        tick(&profiler, &clock, Primitive::Msm, 5_000.0);

        let report = profiler.finish().expect("report should build");
        assert_eq!(report.phases.len(), 2);
        assert_eq!(report.primitives.len(), 1);
        assert!((report.phase("prove").unwrap().ms - 5.0).abs() < 1e-9);
    }

    #[test]
    fn nested_spans_each_record_their_own_duration() {
        let clock = ScriptedClock::new();
        let profiler = profiler(&clock);

        profiler.span("outer", || {
            clock.advance(100.0);
            profiler.span(Primitive::Msm, || {
                clock.advance(500.0);
            });
            clock.advance(200.0);
        });

        assert!((profiler.primitive_us(Primitive::Msm) - 500.0).abs() < f64::EPSILON);
        let report = profiler.finish().expect("report should build");
        // The outer phase saw all 800 us of its body, including the nested one.
        assert!((report.phase("outer").unwrap().ms - 0.8).abs() < 1e-9);
    }

    #[test]
    fn primitive_rows_appear_in_first_use_order_not_enum_order() {
        let clock = ScriptedClock::new();
        let profiler = profiler(&clock);

        tick(&profiler, &clock, Primitive::Other, 1_000.0);
        tick(&profiler, &clock, Primitive::Msm, 1_000.0);

        let report = profiler.finish().expect("report should build");
        let names: Vec<Primitive> = report.primitives.iter().map(|row| row.name).collect();
        assert_eq!(names, vec![Primitive::Other, Primitive::Msm]);
    }

    #[test]
    fn primitive_shares_are_taken_over_attributed_time_not_total() {
        let clock = ScriptedClock::new();
        let profiler = profiler(&clock);

        // 3 ms attributed, 1 ms of the region spent outside any span.
        tick(&profiler, &clock, Primitive::Msm, 2_000.0);
        tick(&profiler, &clock, Primitive::Hash, 1_000.0);
        clock.advance(1_000.0);

        let report = profiler.finish().expect("report should build");
        assert!((report.primitive(Primitive::Msm).unwrap().share - 2.0 / 3.0).abs() < 1e-9);
        assert!((report.primitive(Primitive::Hash).unwrap().share - 1.0 / 3.0).abs() < 1e-9);
        assert!((report.total_ms - 4.0).abs() < 1e-9);
        assert!((report.unattributed_ms() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn nothing_measured_means_no_report_rather_than_zero_shares() {
        let clock = ScriptedClock::new();
        let profiler = profiler(&clock);

        clock.advance(1_000.0);

        assert_eq!(profiler.finish(), Err(ReportError::NoMeasuredTime));
    }

    #[test]
    fn a_phase_only_run_also_produces_no_report() {
        let clock = ScriptedClock::new();
        let profiler = profiler(&clock);

        tick(&profiler, &clock, "setup", 1_000.0);

        assert_eq!(profiler.finish(), Err(ReportError::NoMeasuredTime));
    }

    #[test]
    fn an_open_span_at_finish_time_is_an_error_not_a_silent_undercount() {
        let clock = ScriptedClock::new();
        let profiler = profiler(&clock);

        tick(&profiler, &clock, Primitive::Msm, 1_000.0);
        let guard = profiler.start(Primitive::NttFft);
        clock.advance(500.0);

        let error = profiler.finish().expect_err("an open span must be caught");
        assert_eq!(
            error,
            ReportError::SpanStillOpen {
                name: "ntt_fft".to_owned()
            }
        );

        drop(guard);
        assert!(profiler.finish().is_ok());
    }

    #[test]
    fn the_report_carries_isolation_resolution_and_iterations() {
        let mut clock = ScriptedClock::new();
        clock.isolated = true;
        let profiler = profiler(&clock)
            .with_iterations(64)
            .with_threads(Threads::shared(4));

        tick(&profiler, &clock, Primitive::Msm, 1_000.0);

        let report = profiler.finish().expect("report should build");
        assert!(report.cross_origin_isolated);
        assert!((report.timer_resolution_us - 0.5).abs() < f64::EPSILON);
        assert_eq!(report.iterations, 64);
        assert_eq!(report.threads.count, 4);
        assert!(report.threads.shared_array_buffer);
    }

    #[test]
    fn the_report_is_stamped_with_the_schema_id_and_version() {
        let clock = ScriptedClock::new();
        let profiler = profiler(&clock);
        tick(&profiler, &clock, Primitive::Msm, 1_000.0);

        let report = profiler.finish().expect("report should build");
        assert_eq!(report.schema.as_deref(), Some(crate::schema::SCHEMA_ID));
        assert_eq!(report.report_version, crate::schema::REPORT_VERSION);
    }

    #[test]
    fn setters_are_carried_into_the_report() {
        let clock = ScriptedClock::new();
        let profiler = profiler(&clock)
            .with_memory(Memory::peak_heap_bytes(1_048_576))
            .with_run_manifest_ref("runs/manifest-0001.json")
            .with_iterations(8);

        tick(&profiler, &clock, Primitive::WitnessGen, 1_000.0);

        let report = profiler.finish().expect("report should build");
        assert_eq!(report.memory.peak_heap_bytes, Some(1_048_576));
        assert_eq!(
            report.run_manifest_ref.as_deref(),
            Some("runs/manifest-0001.json")
        );
        assert_eq!(report.iterations, 8);
    }

    #[test]
    fn a_memory_reading_taken_after_the_workload_reaches_the_report() {
        let clock = ScriptedClock::new();
        let profiler = profiler(&clock);

        tick(&profiler, &clock, Primitive::Msm, 1_000.0);
        // The whole point of `record_memory`: a high-water mark is only
        // observable once the work is done, so it cannot go in the builder.
        profiler.record_memory(Memory::peak_heap_bytes(33_554_432));

        let report = profiler.finish().expect("report should build");
        assert_eq!(report.memory.peak_heap_bytes, Some(33_554_432));
    }

    #[test]
    fn the_macro_form_attributes_the_same_as_the_method_form() {
        let clock = ScriptedClock::new();
        let profiler = profiler(&clock);

        crate::span!(&profiler, Primitive::Marshalling, {
            clock.advance(2_000.0);
        });

        assert!((profiler.primitive_us(Primitive::Marshalling) - 2_000.0).abs() < f64::EPSILON);
    }

    #[test]
    fn a_panicking_body_still_records_its_span() {
        let clock = ScriptedClock::new();
        let profiler = profiler(&clock);

        let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            profiler.span(Primitive::Msm, || {
                clock.advance(1_500.0);
                panic!("boom");
            });
        }));

        assert!(panicked.is_err());
        assert!((profiler.primitive_us(Primitive::Msm) - 1_500.0).abs() < f64::EPSILON);
    }

    #[test]
    fn labels_display_as_their_json_spelling() {
        assert_eq!(Label::from(Primitive::NttFft).to_string(), "ntt_fft");
        assert_eq!(Label::from("setup").to_string(), "setup");
        assert_eq!(
            Label::from(String::from("teardown")).to_string(),
            "teardown"
        );
    }

    #[test]
    fn phase_shares_are_relative_to_the_whole_region_and_clamped() {
        let clock = ScriptedClock::new();
        let profiler = profiler(&clock);

        tick(&profiler, &clock, "half", 2_000.0);
        tick(&profiler, &clock, Primitive::Msm, 1_000.0);
        clock.advance(1_000.0);

        let report = profiler.finish().expect("report should build");
        let half = report.phase("half").unwrap();
        assert!((half.share - 0.5).abs() < 1e-9);
    }
}
