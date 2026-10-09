// SPDX-License-Identifier: Apache-2.0

//! The `#[wasm_bindgen]` surface the WebGPU page imports.
//!
//! Thin wrappers, and nothing else. Every decision this surface is involved
//! in is made in [`crate::pipeline`] or in one of the native modules, because
//! this module is `wasm32`-only: whatever is written here is compiled and
//! linted by the `wasm32` CI job and executed by nobody. See the crate docs.
//!
//! # Why the run is `async` here when the harness page's is not
//!
//! [`zkprof_wasm::run_benchmark`] is deliberately synchronous, because an
//! `await` inside a measured region is a measurement of the event loop. That
//! reasoning does not transfer: the region this measures *is* a series of
//! waits on a device. The GPU work happens while the task is parked, so
//! blocking the thread would block the very completion notifications being
//! waited on, and the promise would never settle.

use wasm_bindgen::prelude::*;

use crate::pipeline;
use crate::shader::M31_MUL_WGSL;
use crate::spec::{limits, Spec};
use crate::Error;

/// The kernel source, so the page can show the thing it is timing.
///
/// Not a measurement and not a config knob. A reader who wants to know what
/// was dispatched should not have to open the repository.
#[wasm_bindgen]
#[must_use]
pub fn webgpu_shader() -> String {
    M31_MUL_WGSL.to_owned()
}

/// The knobs the page can set, with their defaults and ceilings, as JSON.
///
/// # Errors
///
/// Serialisation of a struct of numbers cannot fail in practice; the
/// signature is honest about the call rather than assuming.
#[wasm_bindgen]
pub fn webgpu_limits() -> Result<String, JsError> {
    serde_json::to_string(&limits())
        .map_err(Error::from)
        .map_err(JsError::from)
}

/// Runs one plumbing check and returns the result envelope as JSON text.
///
/// Resolves to the [`crate::run::RunResult`] the page renders, which carries
/// the schema-valid report alongside the adapter name, the run's parameters,
/// the byte counts and the below-target verdict.
///
/// # Arguments
///
/// * `elements` — elements per operand buffer. Must be a non-zero multiple of
///   the workgroup size and at most the ceiling from [`webgpu_limits`].
///   Defaults to 65 536.
/// * `dispatches` — kernel launches inside the one submission. Defaults to 1.
///   Raising it is how a caller lifts the run above the clock's noise floor;
///   the report's `iterations` records the number, so a figure is never
///   silently per-launch or silently aggregate.
/// * `seed` — seed for the operands. Defaults to a fixed value, so two runs
///   are comparable.
///
/// # Errors
///
/// Rejects with one of the [`Error`] messages. The one worth calling out is
/// [`Error::Mismatch`]: a run whose output disagrees with the CPU reference
/// produces no report at all, rather than a report about work that did not
/// happen.
#[wasm_bindgen]
pub async fn webgpu_run(
    elements: Option<u32>,
    dispatches: Option<u32>,
    seed: Option<u64>,
) -> Result<String, JsError> {
    let mut spec = Spec::default();
    if let Some(elements) = elements {
        spec = spec.with_elements(elements);
    }
    if let Some(dispatches) = dispatches {
        spec = spec.with_dispatches(dispatches);
    }
    if let Some(seed) = seed {
        spec.seed = seed;
    }

    let result = pipeline::run(spec).await?;
    serde_json::to_string(&result)
        .map_err(Error::from)
        .map_err(JsError::from)
}
