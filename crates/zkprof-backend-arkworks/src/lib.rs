// SPDX-License-Identifier: Apache-2.0

//! Groth16/BN254 backend for zkprof-web.
//!
//! Four pieces:
//!
//! - [`circuit`] — a depth-parameterised Merkle-Poseidon membership circuit.
//! - [`sha256`] — a SHA-256 preimage circuit sized to the csp-benchmarks
//!   canonical input sizes, so a measurement made here is comparable with
//!   the numbers that suite publishes.
//! - [`prover`] — `ark-groth16`'s prover, taken apart so each stage can be
//!   timed, and checked against upstream so that taking it apart cannot
//!   change its answer.
//! - [`bench`] — drives a run, verifies the proof, and emits a
//!   `zkprof-report` document.
//!
//! # What this crate does not claim
//!
//! It contains no benchmark results. Every number it produces comes from a
//! run on the machine it runs on, and the report says which target and clock
//! produced it. Nothing here compares this backend to any other
//! implementation, in this repository or outside it.

#![forbid(unsafe_code)]

pub mod backend;
pub mod bench;
pub mod circuit;
pub mod prover;
pub mod sha256;

pub use bench::{benchmark, digest_public_inputs, Circuit, Config, Error, Outcome, DEFAULT_SEED};
pub use circuit::{
    compress, membership_witness, poseidon_config, MerklePoseidonCircuit, DEFAULT_DEPTH,
};
pub use prover::{prove_with_spans, Proved};
pub use sha256::{
    sha256_digest, Sha256PreimageCircuit, DEFAULT_SHA256_INPUT_BYTES, DIGEST_BYTES,
    SHA256_INPUT_SIZES,
};

// Re-export the backend trait so downstream crates can depend on it
// without importing the zkprof-core types directly.
pub use zkprof_core::backend::{BackendError, CircuitId, ProofArtifact, Span, ZkprofBackend};
