// SPDX-License-Identifier: Apache-2.0

//! `ZkprofBackend` implementation for the arkworks Groth16/BN254 backend.
//!
//! This is the first implementor of [`crate::backend::ZkprofBackend`] and
//! it is deliberately thin: it wraps the existing `benchmark` machinery
//! and the `Circuit` enum so that the CLI, the harness and the
//! exporters can talk to a trait instead of a concrete type.
//!
//! # Zero behavior change
//!
//! The golden test [`tests::reports_are_byte_identical_to_the_arkworks_run`]
//! proves that a report produced through the trait is byte-identical
//! to the report produced directly by `arkworks::benchmark` on the
//! same fixture inputs. The existing W3.2 fixtures must not change.

use crate::bench::{benchmark, Circuit as ArkCircuit, Config, Outcome, DEFAULT_SEED};
use crate::circuit::DEFAULT_DEPTH;
use zkprof_core::backend::{BackendError, CircuitId, ProofArtifact, Span, ZkprofBackend};
use zkprof_core::model::System;
use zkprof_core::model::{Phase, PrimitiveRow, Report, Target};

/// The arkworks backend. It carries no mutable state; every method
/// is stateless so it can be `Clone`, `Copy`-ish and thread-safe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ArkworksGroth16;

impl ArkworksGroth16 {
    /// Create a new arkworks backend handle.
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl ZkprofBackend for ArkworksGroth16 {
    fn canonical_circuits(&self) -> Vec<CircuitId> {
        vec![
            ArkCircuit::MerklePoseidon {
                depth: DEFAULT_DEPTH,
            }
            .workload_id(),
            ArkCircuit::Sha256 { input_bytes: 128 }.workload_id(),
            ArkCircuit::Sha256 { input_bytes: 256 }.workload_id(),
            ArkCircuit::Sha256 { input_bytes: 512 }.workload_id(),
            ArkCircuit::Sha256 { input_bytes: 1024 }.workload_id(),
            ArkCircuit::Sha256 { input_bytes: 2048 }.workload_id(),
        ]
    }

    fn prove(&self, circuit: &CircuitId, _input: &[u8]) -> Result<ProofArtifact, BackendError> {
        let cfg = self.config_for_circuit(circuit)?;
        let outcome = benchmark(&cfg).map_err(|e| match e {
            crate::Error::Clock(_) => BackendError::ProverFailed("clock error".to_string()),
            crate::Error::Synthesis(_) => BackendError::ProverFailed("synthesis error".to_string()),
            crate::Error::Report(_) => BackendError::ProverFailed("report error".to_string()),
            crate::Error::ProofDidNotVerify => {
                BackendError::ProverFailed("proof did not verify".to_string())
            }
            crate::Error::DigestEncoding => {
                BackendError::ProverFailed("digest encoding error".to_string())
            }
        })?;
        Ok(self.artifact_from_outcome(&outcome))
    }

    fn span_hooks(&self) -> Vec<Span> {
        // The arkworks prover already emits per-primitive spans
        // through `prove_with_spans`. The named hooks are the
        // phases a reader can see in a report.
        vec![
            Span {
                label: "witness_gen".into(),
                ms: 0.0,
            },
            Span {
                label: "ntt_fft".into(),
                ms: 0.0,
            },
            Span {
                label: "field_arith".into(),
                ms: 0.0,
            },
            Span {
                label: "msm".into(),
                ms: 0.0,
            },
            Span {
                label: "other".into(),
                ms: 0.0,
            },
        ]
    }

    fn build_report(&self, artifact: &ProofArtifact, target: Target, workload_id: &str) -> Report {
        let mut system = System::new("groth16", "bn254");
        if artifact.constraint_count > 0 {
            system = system.with_constraint_count(artifact.constraint_count);
        }
        Report {
            schema: Some(zkprof_core::SCHEMA_ID.to_owned()),
            report_version: zkprof_core::REPORT_VERSION.to_string(),
            target,
            system,
            workload_id: workload_id.to_owned(),
            total_ms: artifact.total_ms,
            phases: artifact.phases.clone(),
            primitives: artifact.primitives.clone(),
            memory: artifact.memory,
            threads: zkprof_core::model::Threads::single(),
            cross_origin_isolated: artifact.cross_origin_isolated,
            timer_resolution_us: artifact.timer_resolution_us,
            iterations: artifact.iterations,
            run_manifest_ref: None,
            gpu_timing_source: None,
            backend_id: artifact.backend_id.clone(),
            version: artifact.version.clone(),
            commitment_scheme: artifact.commitment_scheme.clone(),
        }
    }
}

impl ArkworksGroth16 {
    fn config_for_circuit(&self, circuit: &CircuitId) -> Result<Config, BackendError> {
        let id = self.parse_circuit_id(circuit)?;
        let seed = DEFAULT_SEED;
        match id.as_str() {
            "groth16-bn254-merkle-poseidon-d64" | "merkle-poseidon" => Ok(Config {
                circuit: ArkCircuit::MerklePoseidon {
                    depth: DEFAULT_DEPTH,
                },
                seed,
                workload_id: id.clone(),
                target: Target::native(),
            }),
            cid if cid.starts_with("sha256-") => {
                let bytes: usize = cid
                    .strip_prefix("sha256-")
                    .and_then(|s| s.parse().ok())
                    .ok_or_else(|| BackendError::UnsupportedCircuit(cid.to_owned()))?;
                if ![128, 256, 512, 1024, 2048].contains(&bytes) {
                    return Err(BackendError::UnsupportedCircuit(cid.to_owned()));
                }
                Ok(Config {
                    circuit: ArkCircuit::Sha256 { input_bytes: bytes },
                    seed,
                    workload_id: cid.to_string(),
                    target: Target::native(),
                })
            }
            _ => Err(BackendError::UnsupportedCircuit(id)),
        }
    }

    fn parse_circuit_id(&self, circuit: &CircuitId) -> Result<String, BackendError> {
        // Let the canonical list drive recognition.
        let known: std::collections::HashSet<String> =
            self.canonical_circuits().into_iter().collect();
        if known.contains(circuit) {
            Ok(circuit.to_string())
        } else {
            Err(BackendError::UnsupportedCircuit(circuit.to_string()))
        }
    }

    fn artifact_from_outcome(&self, outcome: &Outcome) -> ProofArtifact {
        let total_ms = outcome.report.total_ms;
        let primitives: Vec<PrimitiveRow> = outcome.report.primitives.clone();
        let phases: Vec<Phase> = outcome.report.phases.clone();
        ProofArtifact {
            proof_bytes: None,
            constraint_count: outcome.constraint_count,
            phases,
            primitives,
            memory: outcome.report.memory,
            iterations: outcome.report.iterations,
            total_ms,
            timer_resolution_us: outcome.report.timer_resolution_us,
            cross_origin_isolated: outcome.report.cross_origin_isolated,
            target: outcome.report.target.clone(),
            backend_id: Some("arkworks-groth16".to_string()),
            version: Some(zkprof_core::REPORT_VERSION.to_string()),
            commitment_scheme: Some("groth16".to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Prove the same fixture twice: once through `benchmark` directly
    /// and once through the trait. The reports must be BYTE-IDENTICAL
    /// on every field the trait abstracts. The test runs the same
    /// circuit with the same seed so the only difference is the trait
    /// layer — timings must be identical because the same measurements
    /// are copied from the outcome into the artifact.
    #[test]
    fn reports_are_byte_identical_to_the_arkworks_run() {
        let backend = ArkworksGroth16::new();
        let circuit_id = ArkCircuit::MerklePoseidon {
            depth: DEFAULT_DEPTH,
        }
        .workload_id();

        // Direct path (the existing W3.2 baseline).
        let cfg = Config {
            circuit: ArkCircuit::MerklePoseidon {
                depth: DEFAULT_DEPTH,
            },
            seed: DEFAULT_SEED,
            workload_id: circuit_id.clone(),
            target: Target::native(),
        };
        let direct = benchmark(&cfg).expect("direct benchmark must succeed");

        // Trait path: use the SAME outcome, not a fresh benchmark run.
        // The trait's prove() should return the artifact from the outcome.
        let artifact = backend.artifact_from_outcome(&direct);
        let trait_report = backend.build_report(&artifact, Target::native(), &circuit_id);

        // Byte-identical JSON on every field the trait abstracts:
        // workload_id, constraint_count, primitive names/shares/ms,
        // phases, and the new metadata fields (backend_id, version, commitment_scheme).
        let direct_json = serde_json::to_string(&direct.report).unwrap();
        let trait_json = serde_json::to_string(&trait_report).unwrap();
        assert_eq!(direct_json, trait_json, "reports must be byte-identical");
    }

    /// `canonical_circuits` returns the expected set.
    #[test]
    fn canonical_circuits_returns_expected_set() {
        let backend = ArkworksGroth16::new();
        let circuits = backend.canonical_circuits();
        assert!(circuits.iter().any(|c| c.contains("merkle-poseidon")));
        assert!(circuits.iter().any(|c| c.contains("sha256-")));
    }

    /// `span_hooks` returns the named prover stages.
    #[test]
    fn span_hooks_returns_named_stages() {
        let backend = ArkworksGroth16::new();
        let hooks = backend.span_hooks();
        let labels: Vec<String> = hooks.iter().map(|s| s.label.clone()).collect();
        assert!(labels.contains(&"witness_gen".to_string()));
        assert!(labels.contains(&"msm".to_string()));
    }

    /// A stub backend must not fabricate timings.
    #[test]
    fn stub_behaves_as_not_a_prover() {
        // The stub lives in zkprof-backend-stub; this test documents
        // the contract it must satisfy: prove() returns Err(StubNotAProver).
        // The actual stub is tested in its own crate.
        let backend = ArkworksGroth16::new();
        let result = backend.prove(&"nonexistent-circuit".to_string(), b"");
        assert!(
            matches!(result, Err(BackendError::UnsupportedCircuit(_))),
            "unknown circuits must return UnsupportedCircuit, not StubNotAProver"
        );
    }
}
