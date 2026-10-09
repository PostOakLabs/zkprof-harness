// SPDX-License-Identifier: Apache-2.0

//! `ZkprofBackend` implementation that does not prove.
//!
//! The stub backend exists so that the CLI, the harness and the exporters
//! can be exercised against a backend that returns a schema-valid report
//! whose timings are all `0.0` and whose `backend_id`/`version` are `"stub"`.
//! It is the reference for what a "not a prover" backend looks like.
//!
//! # Contract
//!
//! - `canonical_circuits()` returns an empty list.
//! - `prove()` always returns `Err(BackendError::StubNotAProver)`.
//! - `span_hooks()` returns an empty list.
//! - `build_report()` produces a report that passes `Report::validate()` and
//!   whose `backend_id` is `"stub"`, `version` is `"stub"`, and
//!   `commitment_scheme` is `None` (omitted, not `null` — that field is
//!   optional). Timing fields are `0.0`, not `null`: the schema makes
//!   `total_ms`/`timer_resolution_us`/`ms` **required**, so a stub writes a
//!   placeholder `0.0` rather than a value it cannot represent as absent.
//!   (There is no `backend_status` field in the schema or in the code; that
//!   is `backend_id`.)

use std::fmt;
use zkprof_core::backend::{BackendError, CircuitId, ProofArtifact, Span, ZkprofBackend};
use zkprof_core::model::{Memory, Primitive, PrimitiveRow, Report, System, Target, Threads};

/// The stub backend. It carries no state and implements the trait
/// by refusing to prove and emitting null-timing reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StubBackend;

impl StubBackend {
    /// Create a new stub backend handle.
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl ZkprofBackend for StubBackend {
    fn canonical_circuits(&self) -> Vec<CircuitId> {
        Vec::new()
    }

    fn prove(&self, _circuit: &CircuitId, _input: &[u8]) -> Result<ProofArtifact, BackendError> {
        Err(BackendError::StubNotAProver)
    }

    fn span_hooks(&self) -> Vec<Span> {
        Vec::new()
    }

    fn build_report(&self, _artifact: &ProofArtifact, target: Target, workload_id: &str) -> Report {
        Report {
            schema: Some(zkprof_core::SCHEMA_ID.to_owned()),
            report_version: zkprof_core::REPORT_VERSION.to_string(),
            target,
            system: System::new("stub", "stub"),
            workload_id: workload_id.to_owned(),
            total_ms: 0.0,
            phases: Vec::new(),
            primitives: vec![PrimitiveRow {
                name: Primitive::Other,
                ms: 0.0,
                share: 1.0,
            }],
            memory: Memory::unmeasured(),
            threads: Threads::single(),
            cross_origin_isolated: false,
            timer_resolution_us: 0.0,
            iterations: 1,
            run_manifest_ref: None,
            gpu_timing_source: None,
            backend_id: Some("stub".to_string()),
            version: Some("stub".to_string()),
            commitment_scheme: None,
        }
    }
}

impl fmt::Display for StubBackend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "StubBackend")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zkprof_core::backend::BackendError;

    #[test]
    fn canonical_circuits_is_empty() {
        let backend = StubBackend::new();
        assert!(backend.canonical_circuits().is_empty());
    }

    #[test]
    fn prove_returns_stub_not_a_prover() {
        let backend = StubBackend::new();
        let result = backend.prove(&"any-circuit".to_string(), b"");
        assert!(matches!(result, Err(BackendError::StubNotAProver)));
    }

    #[test]
    fn span_hooks_is_empty() {
        let backend = StubBackend::new();
        assert!(backend.span_hooks().is_empty());
    }

    #[test]
    fn build_report_emits_schema_valid_stub_report() {
        let backend = StubBackend::new();
        let artifact = ProofArtifact {
            proof_bytes: None,
            constraint_count: 0,
            phases: Vec::new(),
            primitives: Vec::new(),
            memory: Memory::unmeasured(),
            iterations: 1,
            total_ms: 0.0,
            timer_resolution_us: 0.0,
            cross_origin_isolated: false,
            target: Target::native(),
            backend_id: Some("stub".to_string()),
            version: Some("stub".to_string()),
            commitment_scheme: None,
        };
        let report = backend.build_report(&artifact, Target::native(), "stub-workload");
        // The report must validate
        assert!(report.validate().is_ok());
        // Must carry the stub metadata
        assert_eq!(report.backend_id.as_deref(), Some("stub"));
        assert_eq!(report.version.as_deref(), Some("stub"));
        assert_eq!(report.system.proof_system, "stub");
        assert_eq!(report.system.curve_or_field, "stub");
        // Has a single Other primitive with share 1.0
        assert_eq!(report.primitives.len(), 1);
        assert_eq!(report.primitives[0].name, Primitive::Other);
        assert_eq!(report.primitives[0].share, 1.0);
        assert_eq!(report.iterations, 1);
    }
}
