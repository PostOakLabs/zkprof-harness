// SPDX-License-Identifier: Apache-2.0

//! The `#[wasm_bindgen]` surface the harness page imports.
//!
//! # Who calls this, and from where
//!
//! The harness page runs the workload in a Web Worker, not on the main thread.
//! [`run_benchmark`] is synchronous and can occupy the thread for a minute at
//! default depth; on the main thread that is a frozen tab and, in some
//! browsers, a kill prompt. Blocking a worker costs nothing but the worker.
//!
//! # What is deliberately not here
//!
//! There is no `async` variant. Making [`run_benchmark`] yield would put
//! `await` points inside the measured region, and a measurement that stops
//! for a microtask checkpoint is measuring the event loop. The workload runs
//! straight through; the *caller* gets concurrency by moving to a worker.
//!
//! # Why the result is an envelope and not the report
//!
//! [`run_benchmark`] returns a [`RunResult`], not a bare report. Everything
//! the page would otherwise have to re-derive in JavaScript is computed here
//! instead, from the same values the report was built from.
//!
//! `below_target` in particular is
//! [`zkprof_backend_arkworks::bench::Outcome::below_target`], and duplicating
//! that rule in JavaScript is how a page ends up disagreeing with the CLI
//! about whether a row is above the noise floor.

use serde::Serialize;
use wasm_bindgen::prelude::*;

use zkprof_backend_arkworks::bench::{benchmark, default_workload_id, Config, DEFAULT_SEED};
use zkprof_backend_arkworks::circuit::DEFAULT_DEPTH;
use zkprof_backend_arkworks::{Circuit, DEFAULT_SHA256_INPUT_BYTES, SHA256_INPUT_SIZES};
use zkprof_core::browser::{browser_target, build_target_kind};
use zkprof_core::model::Report;

use crate::Error;

/// Which target this module was built for, as the schema spells it:
/// `wasm-scalar`, `wasm-simd` or `wasm-threads`.
#[wasm_bindgen]
#[must_use]
pub fn target_kind() -> String {
    build_target_kind().as_str().to_owned()
}

/// What this module can see about the host, as a serialised
/// [`zkprof_core::model::Target`].
///
/// Separate from the report so the page can show the deployment context
/// *before* asking for a run that takes a minute.
///
/// # Errors
///
/// Serialisation of a struct built from strings cannot fail in practice; the
/// signature is honest about the call rather than assuming.
#[wasm_bindgen]
pub fn target_json() -> Result<String, JsError> {
    serde_json::to_string(&browser_target())
        .map_err(Error::from)
        .map_err(JsError::from)
}

/// Whether this module was compiled with `+simd128`.
#[wasm_bindgen]
#[must_use]
pub fn simd_enabled() -> bool {
    cfg!(target_feature = "simd128")
}

/// Whether this module was compiled with the `web-threads` feature.
///
/// A `true` here does **not** mean threads are in use. See [`crate::Error`].
#[wasm_bindgen]
#[must_use]
pub fn threads_enabled() -> bool {
    cfg!(feature = "web-threads")
}

/// Everything one run produces, for the harness page to render.
///
/// `report` is the schema-valid artefact and is the only part that is meant
/// to leave the page. The rest is context the page needs to *explain* the
/// report: which rows are below the noise floor, and how the iteration count
/// was chosen.
#[derive(Debug, Clone, Serialize)]
pub struct RunResult {
    /// The schema-valid report, exactly as the CLI would emit it.
    pub report: Report,

    /// Constraints in the circuit, measured during synthesis.
    ///
    /// Not in the report — it is a property of the workload, not of the
    /// measurement — but the page shows it next to the depth because the
    /// depth is meaningless without it.
    pub constraint_count: u64,

    /// Primitive rows whose accumulated time is under the noise floor.
    ///
    /// Computed by [`zkprof_backend_arkworks::bench::Outcome::below_target`].
    /// Empty is the good case; a non-empty list is not an error, but these
    /// rows must not be compared against anything.
    pub below_target: Vec<String>,

    /// How the iteration count was chosen.
    pub plan: PlanSummary,

    /// [`Scaling::describe`] rendered server-side, so the page and the CLI
    /// print the same sentence for the same run.
    pub plan_description: String,
}

/// The parts of [`zkprof_core::scale::Plan`] the page can display.
///
/// Not the whole struct: `per_op_us` and `projected_phase_us` are inputs to
/// the decision, not findings, and `achieved_ratio` is already summarised by
/// `target_met` and `describe`.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct PlanSummary {
    /// Iterations the report was measured over.
    pub iterations: u32,

    /// Clock resolution the decision was based on, µs.
    pub resolution_us: f64,

    /// Whether the run reached the target ratio on a real measurement.
    ///
    /// `false` means the report is honest but coarse. The page says so
    /// rather than showing a number with no error bar implied.
    pub target_met: bool,

    /// Whether the per-op cost behind `target_met` was measured rather than
    /// substituted.
    ///
    /// `target_met` implies this: an unobserved batch cannot have met the
    /// target. It is carried separately because the two answer different
    /// questions when `target_met` is `false` -- `false`/`false` means the
    /// workload is below the clock and the count is meaningless, whereas
    /// `false`/`true` means it was measured and simply did not get far
    /// enough above the clock.
    pub observed: bool,

    /// How many workload calls planning itself consumed, on top of
    /// `iterations`.
    pub calls_spent: u32,
}

/// Runs the workload and returns a [`RunResult`] as JSON text.
///
/// JSON text rather than a live JS object: the page parses it once and hands
/// `report` straight to the download, so the artefact on disk is the same
/// bytes the CLI would have written.
///
/// Blocking by design — see the module docs for why there is no `async`
/// version of this.
///
/// # Arguments
///
/// * `circuit` — which circuit to prove: `"merkle-poseidon"` (the default) or
///   `"sha256"`. `sha256` is a csp-benchmarks canonical circuit, so running it
///   here and natively gives the cross-target delta on a circuit the field
///   already benchmarks.
/// * `depth` — Merkle depth, the circuit-size knob for `merkle-poseidon`.
///   Defaults to [`DEFAULT_DEPTH`]. Must be at least 1.
/// * `bytes` — preimage length for `sha256`. Must be one of
///   [`SHA256_INPUT_SIZES`]; defaults to [`DEFAULT_SHA256_INPUT_BYTES`].
/// * `seed` — seed for the witness, the setup and the prover randomness.
///   Defaults to [`DEFAULT_SEED`], which is what makes two runs comparable.
///
/// # Errors
///
/// Returns a JavaScript `Error` carrying one of the [`Error`] messages: an
/// unknown circuit name, a non-canonical preimage size, a zero or oversized
/// depth, a benchmark or report failure, or [`Error::ThreadsUnimplemented`]
/// when the module was built with `web-threads`.
// `circuit` is owned because `wasm_bindgen` maps a JS string to an owned
// `String`; a borrowed `&str` parameter has no generated glue. The lint is
// right in general and wrong on this side of the boundary.
#[allow(clippy::needless_pass_by_value)]
#[wasm_bindgen]
pub fn run_benchmark(
    circuit: Option<String>,
    depth: Option<u32>,
    bytes: Option<u32>,
    seed: Option<u64>,
) -> Result<String, JsError> {
    // The one path in this crate that is wired but not built. Refusing here
    // is the point: a report produced under a threads flag nobody implemented
    // would look exactly like a real one.
    if cfg!(feature = "web-threads") {
        return Err(JsError::from(Error::ThreadsUnimplemented));
    }

    let circuit = match circuit.as_deref() {
        None | Some("merkle-poseidon") => {
            let depth =
                depth.unwrap_or(u32::try_from(DEFAULT_DEPTH).map_err(|_| Error::DepthTooLarge)?);
            let depth = usize::try_from(depth).map_err(|_| Error::DepthTooLarge)?;
            if depth == 0 {
                return Err(JsError::from(Error::ZeroDepth));
            }
            Circuit::MerklePoseidon { depth }
        }
        Some("sha256") => {
            let bytes = bytes.unwrap_or(
                u32::try_from(DEFAULT_SHA256_INPUT_BYTES).map_err(|_| Error::DepthTooLarge)?,
            );
            let bytes = u64::from(bytes);
            if !SHA256_INPUT_SIZES.contains(&bytes) {
                return Err(JsError::from(Error::NotACanonicalSize(bytes)));
            }
            Circuit::Sha256 {
                input_bytes: usize::try_from(bytes).map_err(|_| Error::DepthTooLarge)?,
            }
        }
        Some(other) => return Err(JsError::from(Error::UnknownCircuit(other.to_owned()))),
    };

    let config = Config {
        workload_id: default_workload_id(&circuit),
        circuit,
        seed: seed.unwrap_or(DEFAULT_SEED),
        target: browser_target(),
    };

    let outcome = benchmark(&config)?;

    // Read before moving `report` out: `below_target` borrows the whole
    // outcome, and the report is the field that gets moved first.
    let below_target: Vec<String> = outcome
        .below_target()
        .into_iter()
        .map(|primitive| primitive.as_str().to_owned())
        .collect();
    let plan_description = outcome.plan.scaling.describe();
    let plan = PlanSummary {
        iterations: outcome.plan.scaling.iterations,
        resolution_us: outcome.plan.scaling.resolution_us,
        target_met: outcome.plan.scaling.target_met,
        observed: outcome.plan.scaling.observed,
        calls_spent: outcome.plan.calls_spent,
    };

    let result = RunResult {
        report: outcome.report,
        constraint_count: outcome.constraint_count,
        below_target,
        plan,
        plan_description,
    };

    serde_json::to_string(&result)
        .map_err(Error::from)
        .map_err(JsError::from)
}
