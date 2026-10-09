// SPDX-License-Identifier: Apache-2.0

//! The wasm-bindgen surface: one exported function per step of the protocol.
//!
//! # Why the host drives
//!
//! Every exported function here is a *step*, not a *run*. The loop is in
//! JavaScript, because the loop is the thing being measured and it has to
//! cross the boundary to do it. See [`crate::bench`].
//!
//! # Why this module owns no logic
//!
//! Everything here is a wrapper: each function reads its arguments, calls
//! [`crate::bench`] or [`crate::host`], and encodes the result as JSON.
//! [`crate::host`] holds the types and the arithmetic, and [`crate::bench`]
//! holds the measurement. The reason is testability — this module is
//! `wasm32`-only, so anything defined here cannot be reached by `cargo test`
//! on a CI machine. Logic kept here would ship untested while the repo still
//! reported a passing suite.
//!
//! # Why the results are envelopes
//!
//! [`marshal_report`] returns the report plus a [`host::Derived`] block of
//! figures computed from it. Those figures — per-call and per-hash
//! microseconds — are the entire point of the bench, and they are arithmetic
//! on `total_ms`, `iterations` and the batch size. Leaving the page to redo
//! that arithmetic is how a page and a CLI end up printing different answers
//! from the same report.
//!
//! [`marshal_size`] likewise returns more than a number, because an iteration
//! count without the resolution it was sized against, and without whether the
//! target was reachable at all, is a number that invites more confidence than
//! it deserves.

use wasm_bindgen::prelude::*;
use zkprof_core::browser::build_target_kind;
use zkprof_core::model::Target;

use crate::bench;
use crate::host::{self, PlanSummary, RunResult, WorkloadSpec};
use crate::shape::Shape;
use crate::Error;

/// Which target this module was built for, as the schema spells it:
/// `wasm-scalar`, `wasm-simd` or `wasm-threads`.
#[wasm_bindgen]
#[must_use]
pub fn marshal_target_kind() -> String {
    build_target_kind().as_str().to_owned()
}

/// The full `target` block every report from this module will carry.
///
/// Exposed so the page can show what the deployment looked like before any
/// measurement is taken, rather than only inside a report.
///
/// # Errors
///
/// Serialisation of this block cannot fail; the `Result` is here so a caller
/// cannot mistake it for a plain string.
#[wasm_bindgen]
pub fn marshal_target_json() -> Result<String, JsError> {
    serde_json::to_string(&target())
        .map_err(Error::from)
        .map_err(JsError::from)
}

/// Whether this module was built with `+simd128`.
#[wasm_bindgen]
#[must_use]
pub fn marshal_simd_enabled() -> bool {
    cfg!(target_feature = "simd128")
}

/// The measured resolution of the clock this module times with, µs.
///
/// Probed, not documented: browsers coarsen `performance.now()` depending on
/// whether the page is cross-origin isolated, and that is a property of the
/// deployment rather than of this code.
///
/// # Errors
///
/// [`Error::Clock`] if no clock could be constructed.
#[wasm_bindgen]
pub fn marshal_timer_resolution_us() -> Result<f64, JsError> {
    Ok(bench::resolution_us()?)
}

/// The workloads the page should run, in order, as a JSON array of
/// [`WorkloadSpec`].
#[wasm_bindgen]
#[must_use]
pub fn marshal_suite() -> String {
    serde_json::to_string(&WorkloadSpec::suite()).unwrap_or_else(|_| "[]".to_owned())
}

/// Starts a workload, replacing any bench already in progress.
///
/// `kind` is `poseidon` or `copy`; `param` is the batch size or the byte
/// count.
///
/// # Errors
///
/// - [`Error::UnknownShape`] for any other `kind`.
/// - [`Error::Shape`] if the payload is empty or over the ceiling.
#[wasm_bindgen]
pub fn marshal_begin(kind: &str, param: u32) -> Result<(), JsError> {
    bench::begin(host::parse_shape(kind, param)?)?;
    Ok(())
}

/// Bytes the caller must pass to each [`marshal_call`].
///
/// # Errors
///
/// [`Error::NotBegun`] if no bench is in progress.
#[wasm_bindgen]
pub fn marshal_payload_len() -> Result<usize, JsError> {
    let shape = current_shape()?;
    Ok(shape.payload_len())
}

/// One boundary crossing: performs the current workload once.
///
/// The host calls this in a loop and times the loop. Returns
/// [`marshal_payload_len`]-dependent output; discarding it is allowed but
/// means the outbound half of the crossing is not being measured.
///
/// # Errors
///
/// - [`Error::NotBegun`] if no bench is in progress.
/// - [`Error::WrongPayloadLen`] if `input` is not the expected length.
#[wasm_bindgen]
pub fn marshal_call(input: &[u8]) -> Result<Vec<u8>, JsError> {
    Ok(bench::call(input)?)
}

/// Sizes the workload from a probe batch and opens the measured region.
///
/// `calls` is how many times the host called [`marshal_call`] while timing
/// `elapsed_ms`. Returns a [`PlanSummary`] as JSON. The probe batch's own
/// measurements are discarded.
///
/// # Errors
///
/// [`Error::NotBegun`] if no bench is in progress.
#[wasm_bindgen]
pub fn marshal_size(calls: u32, elapsed_ms: f64) -> Result<String, JsError> {
    let scaling = bench::size(calls, elapsed_ms)?;
    serde_json::to_string(&PlanSummary::of(&scaling))
        .map_err(Error::from)
        .map_err(JsError::from)
}

/// Closes the measured region and returns the report, as JSON.
///
/// `calls` is how many times the host called [`marshal_call`] while timing
/// `elapsed_ms`. The bench is consumed; [`marshal_begin`] must be called
/// again before another run.
///
/// # Errors
///
/// - [`Error::NotBegun`] if no bench is in progress.
/// - [`Error::Report`] if the assembled report fails validation, which
///   includes the case where nothing measurable was recorded.
#[wasm_bindgen]
pub fn marshal_report(calls: u32, elapsed_ms: f64) -> Result<String, JsError> {
    let report = bench::report(calls, elapsed_ms)?;
    serde_json::to_string(&RunResult::of(report, calls))
        .map_err(Error::from)
        .map_err(JsError::from)
}

/// Throws away the bench in progress, if any.
#[wasm_bindgen]
pub fn marshal_abort() {
    bench::abort();
}

/// The shape of the bench in progress.
fn current_shape() -> Result<Shape, Error> {
    bench::current_shape().ok_or(Error::NotBegun)
}

/// The target block this module reports.
fn target() -> Target {
    zkprof_core::browser::browser_target()
}
