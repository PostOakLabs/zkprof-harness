// SPDX-License-Identifier: Apache-2.0

//! The two workload shapes the boundary bench can run, and the byte counts
//! that go with them.
//!
//! # Why two shapes
//!
//! One shape alone cannot separate the two costs. [`Shape::Poseidon`] crosses
//! the boundary and then does real work; [`Shape::Copy`] crosses the boundary
//! and does essentially none. The difference between them at equal payload
//! size is the work, and the copy figure on its own is the floor the boundary
//! imposes no matter how small the work gets.
//!
//! # Why the payloads are the sizes they are
//!
//! A Poseidon 2:1 compression takes two BN254 scalar field elements and
//! returns one, so one hash moves 64 bytes in and 32 bytes out. A batch of K
//! therefore moves `K * 64` bytes in, and that is exactly what the matching
//! [`Shape::Copy`] payload is set to. Holding bytes-per-hash constant across
//! batch sizes is what makes the per-hash column comparable: any amortisation
//! it shows is amortisation of the *crossing*, not of extra data.

use zkprof_core::model::Primitive;

/// Size of one BN254 scalar field element in canonical little-endian form.
pub const FIELD_BYTES: usize = 32;

/// Bytes in one 2:1 Poseidon compression's input: two field elements.
pub const HASH_INPUT_BYTES: usize = 2 * FIELD_BYTES;

/// Size of the copy baseline's return value: a `u64` sum, little-endian.
pub const COPY_SUM_BYTES: usize = 8;

/// The batch sizes the bench runs, per the work-unit spec.
///
/// 1 is the "one hash per crossing" case the published shape describes as
/// dominated by marshalling; 8, 32 and 128 are the batched cases where it
/// amortises. 32 is included specifically because it is the figure the
/// published comparison states, so the bench can be checked against it.
pub const BATCH_SIZES: [u32; 4] = [1, 8, 32, 128];

/// Largest payload one call will accept, bytes.
///
/// A guard on a caller-supplied length rather than a property of any shape in
/// [`BATCH_SIZES`]: the largest batch is 128 hashes at 64 bytes, or 8 KiB. The
/// ceiling is far above that so the bench is not the thing that stops a larger
/// sweep, and far below anything that could overflow a length computation.
pub const MAX_PAYLOAD_BYTES: usize = 1 << 20;

/// A workload the boundary bench can measure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    /// K Poseidon 2:1 compressions per boundary crossing.
    Poseidon {
        /// How many compressions one call performs.
        batch: u32,
    },
    /// The pure-copy baseline: pass N bytes in, return their sum.
    Copy {
        /// How many bytes one call passes.
        bytes: u32,
    },
}

impl Shape {
    /// Bytes the caller must hand to one call.
    #[must_use]
    pub fn payload_len(self) -> usize {
        match self {
            Self::Poseidon { batch } => {
                usize::try_from(batch).unwrap_or(usize::MAX) * HASH_INPUT_BYTES
            }
            Self::Copy { bytes } => usize::try_from(bytes).unwrap_or(usize::MAX),
        }
    }

    /// Bytes one call returns.
    #[must_use]
    pub fn result_len(self) -> usize {
        match self {
            Self::Poseidon { batch } => usize::try_from(batch).unwrap_or(usize::MAX) * FIELD_BYTES,
            Self::Copy { .. } => COPY_SUM_BYTES,
        }
    }

    /// The `workload_id` this shape reports under.
    ///
    /// The batch size lives here rather than in a report field because the
    /// report schema's `primitives[].name` is a closed enum with a single
    /// `marshalling` variant: there is no `marshalling-batch-8` row to put it
    /// in, and the schema forbids adding one. One report per batch size keeps
    /// every report self-describing and lets `zkprof diff` compare two of
    /// them directly.
    #[must_use]
    pub fn workload_id(self) -> String {
        match self {
            Self::Poseidon { batch } => format!("marshalling-poseidon-batch-{batch}"),
            Self::Copy { bytes } => format!("marshalling-copy-{bytes}b"),
        }
    }

    /// Which primitive the in-wasm work of this shape is attributed to.
    ///
    /// The copy baseline's work *is* the data movement, so it lands in
    /// `marshalling`; the Poseidon compressions land in `hash`. See
    /// [`crate::bench`] for how the rest of each loop is attributed.
    #[must_use]
    pub fn primitive(self) -> Primitive {
        match self {
            Self::Poseidon { .. } => Primitive::Hash,
            Self::Copy { .. } => Primitive::Marshalling,
        }
    }

    /// Rejects a shape whose payload could not be allocated or would wrap a
    /// length computation.
    ///
    /// # Errors
    ///
    /// - [`ShapeError::ZeroPayload`] for a zero batch or byte count.
    /// - [`ShapeError::PayloadTooLarge`] above [`MAX_PAYLOAD_BYTES`].
    pub fn validate(self) -> Result<(), ShapeError> {
        let len = self.payload_len();
        if len == 0 {
            return Err(ShapeError::ZeroPayload);
        }
        if len > MAX_PAYLOAD_BYTES {
            return Err(ShapeError::PayloadTooLarge {
                requested: len,
                max: MAX_PAYLOAD_BYTES,
            });
        }
        Ok(())
    }
}

/// A [`Shape`] the bench refuses to run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShapeError {
    /// A zero batch or byte count crosses the boundary doing nothing, which
    /// measures the call overhead with no payload and is not a comparable
    /// workload.
    ZeroPayload,
    /// The payload is larger than [`MAX_PAYLOAD_BYTES`].
    PayloadTooLarge {
        /// What was asked for, bytes.
        requested: usize,
        /// The ceiling, bytes.
        max: usize,
    },
}

impl std::fmt::Display for ShapeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match *self {
            Self::ZeroPayload => f.write_str("payload must be at least one byte"),
            Self::PayloadTooLarge { requested, max } => {
                write!(
                    f,
                    "payload of {requested} bytes exceeds the {max} byte ceiling"
                )
            }
        }
    }
}

impl std::error::Error for ShapeError {}

/// The pairing the page runs: one Poseidon batch and its matching copy
/// baseline, interleaved so the two columns of the results table line up.
///
/// Copy payloads are `batch * HASH_INPUT_BYTES`, the same number of bytes the
/// Poseidon call carries across the boundary — see the module docs.
#[must_use]
pub fn suite() -> Vec<Shape> {
    let mut shapes = Vec::with_capacity(2 * BATCH_SIZES.len());
    for batch in BATCH_SIZES {
        shapes.push(Shape::Poseidon { batch });
        shapes.push(Shape::Copy {
            bytes: u32::try_from(usize::try_from(batch).unwrap_or(0) * HASH_INPUT_BYTES)
                .unwrap_or(u32::MAX),
        });
    }
    shapes
}

#[cfg(test)]
mod tests {
    use super::{
        suite, Shape, ShapeError, BATCH_SIZES, COPY_SUM_BYTES, FIELD_BYTES, HASH_INPUT_BYTES,
        MAX_PAYLOAD_BYTES,
    };
    use zkprof_core::model::Primitive;

    #[test]
    fn one_hash_moves_two_field_elements_in_and_one_out() {
        let shape = Shape::Poseidon { batch: 1 };
        assert_eq!(shape.payload_len(), 64);
        assert_eq!(shape.result_len(), 32);
    }

    #[test]
    fn a_batch_moves_bytes_in_proportion_to_its_size() {
        for batch in BATCH_SIZES {
            let shape = Shape::Poseidon { batch };
            assert_eq!(shape.payload_len(), usize::try_from(batch).unwrap() * 64);
            assert_eq!(shape.result_len(), usize::try_from(batch).unwrap() * 32);
        }
    }

    #[test]
    fn the_copy_baseline_returns_a_u64_and_nothing_else() {
        let shape = Shape::Copy { bytes: 512 };
        assert_eq!(shape.payload_len(), 512);
        assert_eq!(shape.result_len(), COPY_SUM_BYTES);
    }

    #[test]
    fn each_copy_baseline_carries_exactly_what_its_poseidon_batch_carries() {
        // The invariant that makes the per-hash column comparable: bytes per
        // hash is constant across batch sizes, so only the crossing count
        // changes.
        for batch in BATCH_SIZES {
            let poseidon = Shape::Poseidon { batch };
            let copy = Shape::Copy {
                bytes: u32::try_from(poseidon.payload_len()).unwrap(),
            };
            assert_eq!(copy.payload_len(), poseidon.payload_len());
            assert_eq!(
                poseidon.payload_len(),
                usize::try_from(batch).unwrap() * HASH_INPUT_BYTES
            );
        }
    }

    #[test]
    fn workload_ids_are_distinct_and_name_the_batch_size() {
        let ids: Vec<String> = suite().iter().map(|shape| shape.workload_id()).collect();
        let unique: std::collections::BTreeSet<&String> = ids.iter().collect();
        assert_eq!(unique.len(), ids.len(), "workload ids collide: {ids:?}");
        assert!(ids.contains(&"marshalling-poseidon-batch-32".to_owned()));
        assert!(ids.contains(&"marshalling-copy-2048b".to_owned()));
    }

    #[test]
    fn poseidon_attributes_to_hash_and_copy_to_marshalling() {
        assert_eq!(Shape::Poseidon { batch: 8 }.primitive(), Primitive::Hash);
        assert_eq!(Shape::Copy { bytes: 8 }.primitive(), Primitive::Marshalling);
    }

    #[test]
    fn the_suite_interleaves_each_batch_with_its_baseline() {
        let shapes = suite();
        assert_eq!(shapes.len(), 8);
        let (pairs, remainder) = shapes.as_chunks::<2>();
        assert!(remainder.is_empty(), "the suite should pair up exactly");
        for (index, pair) in pairs.iter().enumerate() {
            assert_eq!(
                pair[0],
                Shape::Poseidon {
                    batch: BATCH_SIZES[index]
                }
            );
            assert_eq!(
                pair[1],
                Shape::Copy {
                    bytes: BATCH_SIZES[index] * u32::try_from(HASH_INPUT_BYTES).unwrap()
                }
            );
        }
    }

    #[test]
    fn a_zero_payload_is_refused() {
        assert_eq!(
            Shape::Poseidon { batch: 0 }.validate(),
            Err(ShapeError::ZeroPayload)
        );
        assert_eq!(
            Shape::Copy { bytes: 0 }.validate(),
            Err(ShapeError::ZeroPayload)
        );
    }

    #[test]
    fn an_oversized_payload_is_refused_and_says_how_oversized() {
        let error = Shape::Copy {
            bytes: u32::try_from(MAX_PAYLOAD_BYTES + 1).unwrap(),
        }
        .validate()
        .expect_err("over the ceiling");
        assert_eq!(
            error,
            ShapeError::PayloadTooLarge {
                requested: MAX_PAYLOAD_BYTES + 1,
                max: MAX_PAYLOAD_BYTES
            }
        );
        assert!(error.to_string().contains("ceiling"));
    }

    #[test]
    fn every_shape_in_the_suite_is_valid() {
        for shape in suite() {
            shape.validate().expect("suite shapes are all runnable");
        }
    }

    #[test]
    fn the_field_size_constant_is_bn254s_scalar_field() {
        // 32 bytes == 254 bits rounded up. If this crate is ever pointed at a
        // different curve, this is the assertion that should fail first.
        assert_eq!(FIELD_BYTES, 32);
        assert_eq!(HASH_INPUT_BYTES, 64);
    }
}
