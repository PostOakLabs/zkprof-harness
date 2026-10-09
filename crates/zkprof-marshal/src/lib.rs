// SPDX-License-Identifier: Apache-2.0

//! The JS↔WASM boundary microbench.
//!
//! This crate answers one question: **what does it cost to cross the boundary
//! between JavaScript and WebAssembly, and how much of that cost goes away if
//! you batch across it?**
//!
//! # Why this exists
//!
//! Every other row in a `zkprof-report` is compute: multi-scalar
//! multiplications, NTTs, hashes. In a browser there is a fourth cost that is
//! not compute and that no compute profiler can see — the crossing itself. A
//! call into wasm is not free, and a call that does one small hash pays the
//! whole crossing to move 64 bytes. Call it once per hash and the crossing
//! dominates; batch 32 hashes into one call and it amortises across all of
//! them.
//!
//! The bench measures that directly, in the shape the effect was published in:
//! one Poseidon call per crossing, then batched ×8, ×32 and ×128, against a
//! pure-copy baseline that crosses the same number of bytes doing nothing.
//!
//! # What it emits
//!
//! One schema-valid `zkprof-report` per workload, with a `marshalling` row.
//! The batch size is in the report's `workload_id`, not in a dedicated field:
//! the schema's `primitives[].name` is a closed enum with a single
//! `marshalling` variant, so there is no `marshalling-batch-8` row to put it
//! in and no room to add one.
//!
//! # What it does not contain
//!
//! **No results.** Every number this crate produces comes from a run on the
//! machine that ran it. Nothing here states how fast a boundary crossing is,
//! how much batching saves, or how any of it compares to anything else —
//! including to the published figures that motivated building it.
//!
//! # Native builds
//!
//! The measurement logic in [`bench`] is target-independent and is unit-tested
//! natively. Only [`bindings`] is `wasm32`-only, because only there is there a
//! host to cross a boundary with.

#![forbid(unsafe_code)]

pub mod bench;
pub mod host;
pub mod shape;

#[cfg(target_arch = "wasm32")]
pub mod bindings;

use std::fmt;

/// Why a bench call failed.
#[derive(Debug)]
pub enum Error {
    /// No bench is in progress: `begin` was never called, or the previous
    /// bench was already consumed by `report`.
    NotBegun,
    /// The shape's payload is empty or over the ceiling.
    Shape(shape::ShapeError),
    /// The payload handed to `call` is not this shape's payload length.
    WrongPayloadLen {
        /// What the shape expects, bytes.
        expected: usize,
        /// What was handed over, bytes.
        actual: usize,
    },
    /// The host named a shape that does not exist.
    ///
    /// Reachable from the wasm surface, where the kind arrives as a string
    /// from JavaScript rather than as a [`shape::Shape`].
    UnknownShape(String),
    /// No clock could be constructed. Only reachable on `wasm32` outside a
    /// JavaScript realm.
    Clock(zkprof_core::ClockError),
    /// The assembled report failed validation.
    Report(zkprof_core::ReportError),
    /// A value could not be serialised for the host.
    ///
    /// Every type this serialises is ours and is structurally finite, so this
    /// is not expected in practice — but the host surface returns strings, and
    /// a failed encoding has to be something other than a panic.
    Json(serde_json::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotBegun => f.write_str(
                "no bench is in progress: call `begin` first, and `report` consumes the bench",
            ),
            Self::Shape(error) => write!(f, "{error}"),
            Self::WrongPayloadLen { expected, actual } => {
                write!(f, "payload is {actual} bytes, this shape takes {expected}")
            }
            Self::UnknownShape(kind) => {
                write!(
                    f,
                    "unknown shape {kind:?}: expected \"poseidon\" or \"copy\""
                )
            }
            Self::Clock(error) => write!(f, "{error}"),
            Self::Report(error) => write!(f, "{error}"),
            Self::Json(error) => write!(f, "could not encode the response: {error}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Shape(error) => Some(error),
            Self::Clock(error) => Some(error),
            Self::Report(error) => Some(error),
            Self::Json(error) => Some(error),
            Self::NotBegun | Self::WrongPayloadLen { .. } | Self::UnknownShape(_) => None,
        }
    }
}

impl From<shape::ShapeError> for Error {
    fn from(error: shape::ShapeError) -> Self {
        Self::Shape(error)
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

#[cfg(test)]
mod tests {
    use super::Error;
    use crate::shape::ShapeError;

    #[test]
    fn a_not_begun_error_says_how_to_recover() {
        let message = Error::NotBegun.to_string();
        assert!(message.contains("begin"), "{message}");
    }

    #[test]
    fn a_wrong_length_error_names_both_lengths() {
        let message = Error::WrongPayloadLen {
            expected: 512,
            actual: 32,
        }
        .to_string();
        assert!(
            message.contains("512") && message.contains("32"),
            "{message}"
        );
    }

    #[test]
    fn errors_carry_their_sources() {
        use std::error::Error as _;

        let error = Error::Shape(ShapeError::ZeroPayload);
        assert!(error.source().is_some());

        let error = Error::Report(zkprof_core::ReportError::SpanStillOpen {
            name: "hash".to_owned(),
        });
        assert!(error.source().is_some());
        assert!(error.to_string().contains("hash"));
    }
}
