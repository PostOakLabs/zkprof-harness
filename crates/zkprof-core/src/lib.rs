// SPDX-License-Identifier: Apache-2.0

//! Timing attribution core for zkprof-web.
//!
//! This crate is the shared substrate the backend crates build on. It knows
//! nothing about proving systems; it knows how to attribute wall time to
//! labels and how to describe the clock that did the measuring.
//!
//! ```
//! use zkprof_core::clock::platform_clock;
//! use zkprof_core::model::{Primitive, System, Target};
//! use zkprof_core::profiler::Profiler;
//!
//! let clock = platform_clock().expect("a clock");
//! let profiler = Profiler::new(
//!     clock.as_ref(),
//!     Target::native(),
//!     System::new("groth16", "bn254").with_constraint_count(16_384),
//!     "demo-workload",
//! );
//!
//! profiler.span(Primitive::Msm, || { /* multi-scalar multiplication */ });
//! profiler.span(Primitive::Other, || { /* everything else */ });
//!
//! // Would be Err(NoMeasuredTime) here: the spans above measure zero.
//! let _ = profiler.finish();
//! ```
//!
//! # The three things every caller must get right
//!
//! 1. **Measure the clock, do not assume it.** [`clock::probe_resolution`]
//!    exists because browsers coarsen `performance.now()` depending on whether
//!    the page is cross-origin isolated. Every report carries the measured
//!    figure and the isolation flag.
//! 2. **Scale iterations to the clock.** [`scale::scale_iterations`] returns a
//!    count that puts each phase at least 1000× above the resolution, and says
//!    so when it cannot.
//! 3. **Do not invent attribution.** Work that is not inside a span is not
//!    attributed to any primitive. It stays visible as
//!    [`model::Report::unattributed_ms`] rather than being folded into
//!    [`model::Primitive::Other`].

#![forbid(unsafe_code)]

pub mod backend;
pub mod clock;
pub mod csp;
pub mod fibrace;
pub mod memory;
pub mod model;
pub mod profiler;
pub mod scale;
mod schema;

pub mod browser;

pub use clock::{platform_clock, Clock, ClockError, Resolution, DEFAULT_PROBE_SAMPLES};
pub use csp::{
    build_document, build_measurement, detect_target, rfc3339_utc, CspDocument, CspMeasurement,
    CspMetadata, CspSystem, ExportError, BYTE_INPUTS_FULL, CANONICAL_TARGETS, CSP_UPSTREAM_COMMIT,
    CSP_UPSTREAM_DATE, CSP_UPSTREAM_URL, FIELD_ELEMENT_INPUTS_FULL, UNMAPPABLE_FIELDS,
};
pub use memory::linear_memory_bytes;
pub use model::{
    Memory, Phase, Primitive, PrimitiveRow, Report, ReportError, System, Target, TargetKind,
    Threads, UnknownVariant, SHARE_TOLERANCE,
};
pub use profiler::{Label, Profiler, SpanGuard};
pub use scale::{plan_iterations, scale_iterations, Scaling, MAX_ITERATIONS, TARGET_RATIO};
pub use schema::{REPORT_VERSION, SCHEMA_ID, SCHEMA_JSON, SCHEMA_SHA256};

#[cfg(not(target_arch = "wasm32"))]
pub use clock::InstantClock;

#[cfg(target_arch = "wasm32")]
pub use clock::PerformanceClock;
