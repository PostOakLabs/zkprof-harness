// SPDX-License-Identifier: Apache-2.0

//! WebGPU plumbing verification for zkprof-web (W3.5).
//!
//! This crate answers one question: **can a `zkprof` target be a GPU, and
//! does the whole path to it work?** It acquires an adapter and a device,
//! compiles a compute shader, dispatches an M31 multiplication over a buffer,
//! reads the product back, checks it against a CPU reference, and reports the
//! five steps through the ordinary span/report path.
//!
//! # What it is not
//!
//! **It is not a benchmark, and it contains no performance claims.** The
//! kernel is deliberately trivial: pointwise multiplication in the
//! Mersenne-31 field, the smallest field whose reduction is a fold rather
//! than a division. Its job is to be obviously correct so that a plumbing
//! failure cannot be mistaken for a kernel bug. Nothing here says how fast
//! M31 multiplication is on a GPU, how a GPU compares to wasm for any
//! primitive, or whether any of it is worth doing.
//!
//! # What "the plumbing" means
//!
//! Five steps, timed separately, each of which can fail on its own:
//!
//! | phase | what it covers |
//! |-------|---------------|
//! | `device-acquire` | instance, adapter request, device and queue request |
//! | `upload` | buffer creation and the two writes that fill the operands |
//! | `compile` | shader module, layouts, compute pipeline, bind group |
//! | `dispatch` | encode, submit, wait for the device to say it finished |
//! | `readback` | map the staging buffer and copy it into host memory |
//!
//! Only `dispatch` is attributed to a primitive, and see [`run`] for what
//! that figure is and is not. In short: when the adapter offers WebGPU's
//! optional `timestamp-query` feature, a `GPUQuerySet` brackets the compute
//! pass and the attributed figure is the resolved timestamp delta; when it
//! does not, the figure is the wall-clock window and is an upper bound on the
//! kernel rather than a measurement of it. [`timing`] states what the
//! timestamp figure cannot see, which is why both numbers ship whenever both
//! exist.
//!
//! The rest is deliberately left unattributed: it was measured, but calling
//! device acquisition "field arithmetic" would be a lie about the same kind a
//! report exists to avoid.
//!
//! # A run checks its own answer
//!
//! The device's output is compared, element by element, against a CPU
//! implementation of the same kernel before a report is allowed to exist. A
//! disagreement returns [`Error::Mismatch`] naming the first offending index
//! and both values. This is the same rule W3.2 applies to its proof: taking
//! a system apart to time it risks changing its answer, so the answer is
//! checked before the timing is published.
//!
//! # Why `wgpu` is a `wasm32`-only dependency
//!
//! `wgpu` is declared under `[target.'cfg(target_arch = "wasm32")'.dependencies]`
//! in `Cargo.toml`, so a native build does not see it at all. That is a
//! response to two facts:
//!
//! 1. **The native build cannot be built on every machine this repository
//!    has to build on.** `wgpu`'s native backends pull `windows-*` crates
//!    that need `dlltool.exe`, which a MinGW-less Rust install does not
//!    ship. Installing one is a machine-wide change, and this is a shared
//!    machine.
//! 2. **Everything here runs in a browser.** The browser's own WebGPU
//!    implementation is the backend; the native ones are not on any path
//!    this crate takes.
//!
//! The consequence is that [`pipeline`] and [`bindings`] are compiled and
//! linted — the CI gate builds `wasm32-unknown-unknown` in both profiles —
//! but never *executed* by a test. So everything that can be reasoned about
//! without a device lives outside them and is tested natively: [`m31`] (the
//! arithmetic, cross-checked against a widened implementation), [`spec`]
//! (what a run is and what its operands are), [`shader`] (the WGSL source,
//! parsed and validated by `naga` in an ordinary unit test), [`completion`]
//! (the callback-to-future bridge), [`run`] (the phase names and the report
//! the phases assemble into) and [`timing`] (the arithmetic that turns two
//! GPU counter readings into a duration, including what happens when the
//! counter wraps).
//!
//! # Native builds
//!
//! On a native target this crate compiles to [`spec`], [`m31`], [`shader`],
//! [`completion`], [`adapter`], [`run`], [`timing`] and [`Error`], and
//! nothing else.

#![forbid(unsafe_code)]

pub mod adapter;
pub mod completion;
pub mod m31;
pub mod run;
pub mod shader;
pub mod spec;
pub mod timing;

#[cfg(target_arch = "wasm32")]
pub mod bindings;
#[cfg(target_arch = "wasm32")]
pub mod pipeline;

use std::fmt;

/// Why a plumbing run failed.
#[derive(Debug)]
pub enum Error {
    /// The requested run is not runnable: see [`spec::SpecError`].
    Spec(spec::SpecError),
    /// The platform exposed no WebGPU adapter.
    ///
    /// Distinct from a device request failure because the remedy is
    /// different: there is nothing to retry, and the browser either offers
    /// WebGPU or it does not.
    NoAdapter,
    /// The adapter refused a device.
    #[cfg(target_arch = "wasm32")]
    Device(wgpu::RequestDeviceError),
    /// The shader did not compile.
    ///
    /// Carries the implementation's diagnostic text. It is a string rather
    /// than a structured error because the diagnostics are the useful part
    /// and `wgpu` already renders them.
    Shader(String),
    /// The readback buffer could not be mapped.
    #[cfg(target_arch = "wasm32")]
    Map(wgpu::BufferAsyncError),
    /// A view over the already-mapped readback buffer was refused.
    ///
    /// Distinct from [`Error::Map`]: that reports the mapping *request* being
    /// rejected, this reports a *view* over a successfully mapped range being
    /// refused. `wgpu` returns the two from different calls, so collapsing
    /// them would lose which stage failed.
    #[cfg(target_arch = "wasm32")]
    View(wgpu::MapRangeError),
    /// The device handed back a buffer whose length is not a whole number
    /// of four-byte elements.
    RaggedReadback {
        /// Bytes the staging buffer held.
        bytes: usize,
    },
    /// The device handed back a different number of elements than were
    /// uploaded.
    WrongElementCount {
        /// Elements read back.
        elements: usize,
        /// Elements uploaded.
        expected: usize,
    },
    /// The device's output disagreed with the CPU reference.
    ///
    /// This is the check that makes the run worth anything: without it a
    /// silent mistranslation in the kernel would be published as a timing.
    Mismatch {
        /// Index of the first element that differed.
        index: usize,
        /// What the CPU reference computed.
        expected: u32,
        /// What the device produced.
        actual: u32,
    },
    /// No clock could be constructed.
    Clock(zkprof_core::ClockError),
    /// The assembled report failed validation.
    Report(zkprof_core::ReportError),
    /// A value could not be serialised for the host.
    Json(serde_json::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Spec(error) => write!(f, "{error}"),
            Self::NoAdapter => f.write_str(
                "no WebGPU adapter is available; the browser did not expose one, or \
                 WebGPU is disabled for this origin",
            ),
            #[cfg(target_arch = "wasm32")]
            Self::Device(error) => write!(f, "the adapter refused a device: {error}"),
            Self::Shader(diagnostic) => write!(f, "the kernel did not compile:\n{diagnostic}"),
            #[cfg(target_arch = "wasm32")]
            Self::Map(error) => write!(f, "the readback buffer could not be mapped: {error}"),
            #[cfg(target_arch = "wasm32")]
            Self::View(error) => {
                write!(f, "the mapped readback buffer could not be viewed: {error}")
            }
            Self::RaggedReadback { bytes } => write!(
                f,
                "the device returned {bytes} bytes, which is not a whole number of \
                 four-byte elements"
            ),
            Self::WrongElementCount { elements, expected } => write!(
                f,
                "the device returned {elements} elements where {expected} were uploaded"
            ),
            Self::Mismatch {
                index,
                expected,
                actual,
            } => write!(
                f,
                "the device disagrees with the CPU reference at element {index}: \
                 expected {expected}, got {actual}",
            ),
            Self::Clock(error) => write!(f, "{error}"),
            Self::Report(error) => write!(f, "{error}"),
            Self::Json(error) => write!(f, "could not encode the response: {error}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Spec(error) => Some(error),
            #[cfg(target_arch = "wasm32")]
            Self::Device(error) => Some(error),
            #[cfg(target_arch = "wasm32")]
            Self::Map(error) => Some(error),
            #[cfg(target_arch = "wasm32")]
            Self::View(error) => Some(error),
            Self::Clock(error) => Some(error),
            Self::Report(error) => Some(error),
            Self::Json(error) => Some(error),
            Self::NoAdapter
            | Self::Shader(_)
            | Self::RaggedReadback { .. }
            | Self::WrongElementCount { .. }
            | Self::Mismatch { .. } => None,
        }
    }
}

impl From<spec::SpecError> for Error {
    fn from(error: spec::SpecError) -> Self {
        Self::Spec(error)
    }
}

impl From<zkprof_core::ClockError> for Error {
    fn from(error: zkprof_core::ClockError) -> Self {
        Self::Clock(error)
    }
}

impl From<zkprof_core::ReportError> for Error {
    fn from(error: zkprof_core::ReportError) -> Self {
        Self::Report(error)
    }
}

impl From<serde_json::Error> for Error {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

#[cfg(target_arch = "wasm32")]
impl From<wgpu::RequestDeviceError> for Error {
    fn from(error: wgpu::RequestDeviceError) -> Self {
        Self::Device(error)
    }
}

#[cfg(target_arch = "wasm32")]
impl From<wgpu::BufferAsyncError> for Error {
    fn from(error: wgpu::BufferAsyncError) -> Self {
        Self::Map(error)
    }
}

#[cfg(target_arch = "wasm32")]
impl From<wgpu::MapRangeError> for Error {
    fn from(error: wgpu::MapRangeError) -> Self {
        Self::View(error)
    }
}

#[cfg(test)]
mod tests {
    use super::Error;
    use crate::spec::SpecError;

    #[test]
    fn a_mismatch_names_the_index_and_both_values() {
        let message = Error::Mismatch {
            index: 4096,
            expected: 12345,
            actual: 67890,
        }
        .to_string();

        assert!(message.contains("4096"), "{message}");
        assert!(message.contains("12345"), "{message}");
        assert!(message.contains("67890"), "{message}");
    }

    #[test]
    fn no_adapter_says_there_is_nothing_to_retry() {
        let message = Error::NoAdapter.to_string();
        assert!(message.contains("no WebGPU adapter"), "{message}");
    }

    #[test]
    fn a_ragged_readback_names_the_byte_count() {
        let message = Error::RaggedReadback { bytes: 4097 }.to_string();
        assert!(message.contains("4097"), "{message}");
    }

    #[test]
    fn a_wrong_element_count_names_both_counts() {
        let message = Error::WrongElementCount {
            elements: 65535,
            expected: 65536,
        }
        .to_string();
        assert!(
            message.contains("65535") && message.contains("65536"),
            "{message}"
        );
    }

    #[test]
    fn a_shader_failure_carries_the_diagnostic() {
        let message = Error::Shader("error: unexpected token".to_owned()).to_string();
        assert!(message.contains("unexpected token"), "{message}");
    }

    #[test]
    fn errors_carry_their_sources() {
        use std::error::Error as _;

        let error = Error::Spec(SpecError::ZeroElements);
        assert!(error.source().is_some());

        let error = Error::Report(zkprof_core::ReportError::SpanStillOpen {
            name: "dispatch".to_owned(),
        });
        assert!(error.source().is_some());
        assert!(error.to_string().contains("dispatch"));
    }
}
