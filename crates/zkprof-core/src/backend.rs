// SPDX-License-Identifier: Apache-2.0

//! Backend abstraction for zkprof-web.
//!
//! `ZkprofBackend` is the seam that makes zkprof backend-neutral: every
//! proving backend this repository supports implements it, and the CLI,
//! the harness and the exporters call through it rather than reaching
//! into a concrete crate. Adding a new backend (Stwo, Ligetron, a
//! browser-native prover) is adding an impl, not editing every call site.
//!
//! # Design principles
//!
//! - **Zero behavior change for the existing implementor.** The arkworks
//!   backend is the first impl and its reports remain byte-identical to
//!   the W3.2 fixtures.
//! - **Stubs are unmistakable.** `zkprof-backend-stub` emits a report
//!   whose `backend_status` field is `"stub"` and whose timings are
//!   `null`.
//! - **No fabricated numbers.** A backend must not invent proof sizes,
//!   verification times or security claims it did not measure.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::model::{Memory, Phase, PrimitiveRow, Report, Target};

/// A named circuit the backend knows how to prove.
pub type CircuitId = String;

/// A single span of attributed time, the same shape the profiler emits.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Span {
    /// Label of the measured region.
    pub label: String,
    /// Wall time of the region, milliseconds.
    pub ms: f64,
}

/// A proof produced by a backend together with the metadata the report needs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProofArtifact {
    /// The serialized proof bytes. Absent when the backend does not
    /// expose the raw proof.
    pub proof_bytes: Option<Vec<u8>>,
    /// Number of constraints in the circuit that was proved.
    pub constraint_count: u64,
    /// The phases the prover passed through, with their attributed time.
    pub phases: Vec<Phase>,
    /// Primitive-level attribution of the proving work.
    pub primitives: Vec<PrimitiveRow>,
    /// Heap high-water mark, when measured.
    pub memory: Memory,
    /// Number of repetitions that were measured.
    pub iterations: u32,
    /// Total wall time of the proving region, milliseconds.
    pub total_ms: f64,
    /// Measured minimum observable nonzero delta of the clock source, µs.
    pub timer_resolution_us: f64,
    /// Value of the platform's `crossOriginIsolated` flag.
    pub cross_origin_isolated: bool,
    /// Target the workload ran on.
    pub target: Target,
    /// Identity of the proving backend that produced this artifact.
    pub backend_id: Option<String>,
    /// Backend implementation version, if the backend exposes one.
    pub version: Option<String>,
    /// Polynomial commitment scheme the backend uses, if known.
    pub commitment_scheme: Option<String>,
}

/// A proving backend.
///
/// Implementors must keep the existing reports byte-identical when the
/// inputs are identical. A backend that cannot produce a measurement
/// should return a report whose timings are `null` and whose
/// `backend_status` is `"stub"` rather than fabricating a number.
pub trait ZkprofBackend: fmt::Debug + Send + Sync {
    /// Circuit IDs this backend can prove. The order is the canonical order.
    fn canonical_circuits(&self) -> Vec<CircuitId>;

    /// Prove `circuit`, returning the artifact and the final report.
    ///
    /// Implementors that wrap an external prover must return a report
    /// that passes [`Report::validate`] and that is byte-identical to a
    /// Returns the [`ProofArtifact`] for `circuit`, or [`BackendError`] if the
    /// circuit is unsupported or the prover fails. A stub backend returns
    /// [`BackendError::StubNotAProver`].
    ///
    /// # Errors
    ///
    /// Returns [`BackendError::UnsupportedCircuit`] when `circuit` is not
    /// in the backend's [`canonical_circuits`] list, and
    /// [`BackendError::ProverFailed`] when the underlying prover errors.
    fn prove(&self, circuit: &CircuitId, input: &[u8]) -> Result<ProofArtifact, BackendError>;

    /// Return the span-instrumentation hooks this backend exposes.
    ///
    /// The caller can attach these to a [`crate::profiler::Profiler`] so
    /// that the backend's internal stages appear as named phases in the
    /// report.
    fn span_hooks(&self) -> Vec<Span>;

    /// Build a [`Report`] from a [`ProofArtifact`].
    ///
    /// The fields `backend_id`, `version` and `commitment_scheme` come
    /// straight from the artifact; the timing fields come from the
    /// artifact's primitive rows. A stub backend must still produce a
    /// schema-valid report — it does that by emitting null timings and
    /// marking `backend_status`.
    fn build_report(&self, artifact: &ProofArtifact, target: Target, workload_id: &str) -> Report;
}

/// Why a backend refused to prove.
#[derive(Debug, Clone, PartialEq)]
pub enum BackendError {
    /// The circuit is not supported by this backend.
    UnsupportedCircuit(CircuitId),
    /// The prover itself failed (synthesis, setup, verification).
    ProverFailed(String),
    /// The backend is a stub and did not perform a real proof.
    StubNotAProver,
}

impl fmt::Display for BackendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedCircuit(id) => write!(f, "unsupported circuit: {id}"),
            Self::ProverFailed(msg) => write!(f, "prover failed: {msg}"),
            Self::StubNotAProver => write!(f, "this backend is a stub, not a prover"),
        }
    }
}

impl std::error::Error for BackendError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proof_artifact_serializes() {
        let artifact = ProofArtifact {
            proof_bytes: None,
            constraint_count: 15_553,
            phases: vec![Phase {
                name: "setup".to_owned(),
                ms: 1.0,
                share: 0.1,
            }],
            primitives: vec![PrimitiveRow {
                name: crate::model::Primitive::Msm,
                ms: 7.5,
                share: 0.75,
            }],
            memory: Memory::unmeasured(),
            iterations: 1,
            total_ms: 10.0,
            timer_resolution_us: 0.1,
            cross_origin_isolated: false,
            target: Target::native(),
            backend_id: None,
            version: None,
            commitment_scheme: None,
        };
        let json = serde_json::to_string(&artifact).unwrap();
        assert!(json.contains("constraint_count"));
    }
}
