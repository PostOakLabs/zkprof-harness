// SPDX-License-Identifier: Apache-2.0

//! Browser entry point for zkprof-web.
//!
//! This crate is the wasm32 half of the harness: it exposes the same
//! [`zkprof-backend-arkworks`] workload the CLI runs, to JavaScript, through
//! wasm-bindgen. It contains no measurement logic of its own — it decides
//! *which* target a build reports, hands the workload to the backend, and
//! returns the resulting report as JSON.
//!
//! # Build profiles
//!
//! | profile | how | `target.kind` reported |
//! |---------|-----|------------------------|
//! | scalar  | `cargo build --target wasm32-unknown-unknown` | `wasm-scalar` |
//! | simd    | the same, with `RUSTFLAGS="-C target-feature=+simd128"` | `wasm-simd` |
//! | threads | the same, with `--features web-threads` | `wasm-threads` — **unimplemented** |
//!
//! The kind comes from `cfg`, never from a runtime check, so a scalar build
//! cannot describe itself as a simd one. See
//! [`zkprof_core::browser::build_target_kind`].
//!
//! # Threads
//!
//! The `web-threads` feature is plumbed and not implemented. Turning it on
//! changes the reported `target.kind` and makes the exported `run_benchmark`
//! refuse every call with [`Error::ThreadsUnimplemented`]. That is the whole
//! feature, and it is deliberate: the alternative is a flag that silently
//! produces a single-threaded report labelled `wasm-threads`.
//!
//! # Native builds
//!
//! Everything browser-facing is `cfg`-gated on `wasm32`. On a native target
//! this crate compiles to [`build_target_kind`] and [`Error`] and nothing
//! else, so `cargo test --workspace` and `clippy --workspace` keep working
//! without a wasm toolchain.

use std::fmt;

#[cfg(target_arch = "wasm32")]
pub mod bindings;

#[cfg(target_arch = "wasm32")]
pub use bindings::{run_benchmark, simd_enabled, target_json, target_kind, threads_enabled};
pub use zkprof_core::browser::{build_target_kind, cpu_description};

/// Why a browser call failed.
///
/// Mirrors the CLI's error surface rather than reusing it: the CLI writes to
/// stderr and returns an [`std::process::ExitCode`], whereas this has to cross
/// into JavaScript as a thrown `Error` with a readable message.
#[derive(Debug)]
pub enum Error {
    /// The requested depth does not fit the address space this build has.
    DepthTooLarge,
    /// A Merkle depth of 0 is not a circuit.
    ZeroDepth,
    /// The workload itself failed, or the report it produced did not validate.
    Benchmark(zkprof_backend_arkworks::bench::Error),
    /// The report could not be serialised.
    Json(serde_json::Error),
    /// The circuit name is not one this module can build.
    UnknownCircuit(String),
    /// A preimage size csp-benchmarks does not measure.
    ///
    /// Same rule as the CLI refuses under: a size the exporter would reject is
    /// not a size worth spending minutes measuring in a browser tab.
    NotACanonicalSize(u64),
    /// The module was built with `web-threads`, which is plumbed but not built.
    ///
    /// Reported rather than quietly ignored. A `wasm-threads` report from a
    /// single-threaded prover would be indistinguishable from a real one once
    /// it left the page.
    ThreadsUnimplemented,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DepthTooLarge => f.write_str("depth does not fit this build's pointer width"),
            Self::ZeroDepth => f.write_str("depth must be at least 1"),
            Self::Benchmark(error) => write!(f, "{error}"),
            Self::Json(error) => write!(f, "could not serialise the report: {error}"),
            Self::UnknownCircuit(name) => write!(
                f,
                "unknown circuit {name:?}; expected \"merkle-poseidon\" or \"sha256\""
            ),
            Self::NotACanonicalSize(bytes) => {
                write!(f, "preimage size {bytes} is not a csp-benchmarks byte size")
            }
            Self::ThreadsUnimplemented => f.write_str(
                "this module was built with the `web-threads` feature, which wires the \
                 build plumbing but does not implement a worker pool; rebuild without \
                 it to get a single-threaded `wasm-scalar` or `wasm-simd` report",
            ),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Benchmark(error) => Some(error),
            Self::Json(error) => Some(error),
            Self::DepthTooLarge
            | Self::ZeroDepth
            | Self::ThreadsUnimplemented
            | Self::UnknownCircuit(_)
            | Self::NotACanonicalSize(_) => None,
        }
    }
}

impl From<zkprof_backend_arkworks::bench::Error> for Error {
    fn from(error: zkprof_backend_arkworks::bench::Error) -> Self {
        Self::Benchmark(error)
    }
}

impl From<serde_json::Error> for Error {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

// There is deliberately no `impl From<Error> for JsError` here. wasm-bindgen
// already ships `impl<E: std::error::Error> From<E> for JsError`, which covers
// this type, and writing it out by hand is a coherence error. The blanket impl
// uses `Display`, so the messages above are what JavaScript sees.

#[cfg(test)]
mod tests {
    use super::{build_target_kind, Error};
    use zkprof_core::model::TargetKind;

    #[test]
    fn errors_name_their_cause_in_the_display_string() {
        let message = Error::ZeroDepth.to_string();
        assert!(message.contains("at least 1"), "{message}");

        let message = Error::ThreadsUnimplemented.to_string();
        assert!(message.contains("web-threads"), "{message}");
        assert!(message.contains("not implement"), "{message}");
    }

    #[test]
    fn a_benchmark_error_carries_its_source() {
        use std::error::Error as _;

        let error = Error::Benchmark(zkprof_backend_arkworks::bench::Error::ProofDidNotVerify);
        assert!(error.source().is_some());
        assert!(error.to_string().contains("does not verify"));
    }

    #[test]
    fn the_kind_this_build_reports_is_a_schema_spelling() {
        let kind = build_target_kind();
        assert_eq!(kind.as_str().parse::<TargetKind>().unwrap(), kind);
    }
}
