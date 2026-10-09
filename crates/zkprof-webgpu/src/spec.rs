// SPDX-License-Identifier: Apache-2.0

//! What one plumbing run is, and the operands it runs on.
//!
//! This module holds everything about a run that can be decided without a
//! device: how many elements, how many dispatches, what the buffers contain,
//! and what the resulting report calls itself. None of it touches `wgpu`, so
//! all of it is exercised by the native test suite.
//!
//! # Why the operand values are reproducible
//!
//! [`Spec::inputs`] derives both operands from `seed` with a fixed generator.
//! A run is only comparable to another run if the work is identical, and
//! random input would make a wrong GPU result indistinguishable from a
//! different one. The values are pseudorandom rather than a ramp so that a
//! kernel that only handles small values correctly still fails.

use crate::m31;
use serde::Serialize;

/// Threads per workgroup.
///
/// Must match the `@workgroup_size` in `shaders/m31_mul.wgsl`. They are two
/// spellings of the same fact in two languages, so a test in [`crate::shader`]
/// reads the shader source and asserts the literal is this constant. That is
/// the cheapest available guard against the two drifting apart, and the drift
/// would be silent: a mismatched dispatch either leaves elements unprocessed
/// or reads past the end of a buffer.
pub const WORKGROUP_SIZE: u32 = 64;

/// Elements in each operand buffer by default: 65 536, i.e. 256 KiB per
/// buffer at four bytes an element.
///
/// Large enough that a dispatch is not dominated by launch latency on any
/// plausible adapter, small enough that the run is a plumbing check and not a
/// benchmark. There is no performance claim resting on this number.
pub const DEFAULT_ELEMENTS: u32 = 1 << 16;

/// Ceiling on [`Spec::elements`]: 2²⁴, i.e. 64 MiB per operand buffer.
///
/// A guard against a page that asks for more memory than the device will
/// grant and gets an allocation failure with no explanation.
pub const MAX_ELEMENTS: u32 = 1 << 24;

/// Ceiling on [`Spec::dispatches`].
pub const MAX_DISPATCHES: u32 = 4096;

/// Seed [`Spec`] defaults to.
pub const DEFAULT_SEED: u64 = 0x5DEE_CE66_D9EF_2027;

/// Bytes one element occupies in a buffer.
///
/// `array<u32>` in WGSL has a stride of four, so this is fixed by the shader
/// rather than chosen here.
pub const ELEMENT_BYTES: u64 = 4;

/// One plumbing run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Spec {
    /// Elements in each of the three buffers. Must be a multiple of
    /// [`WORKGROUP_SIZE`] so no workgroup is left half-populated.
    pub elements: u32,
    /// How many times the kernel is dispatched inside the one submission.
    ///
    /// More than one exists so a caller can lift the measurement above the
    /// clock's noise floor. It does not make a run more of a measurement:
    /// `iterations` in the report records this number, so a reader can always
    /// see whether a figure is one launch or a thousand.
    pub dispatches: u32,
    /// Seed for the operands.
    pub seed: u64,
}

impl Default for Spec {
    fn default() -> Self {
        Self {
            elements: DEFAULT_ELEMENTS,
            dispatches: 1,
            seed: DEFAULT_SEED,
        }
    }
}

/// Why a [`Spec`] cannot be run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpecError {
    /// `elements` was zero.
    ZeroElements,
    /// `elements` is not a whole number of workgroups.
    NotMultipleOfWorkgroup {
        /// What was asked for.
        elements: u32,
        /// What it would have to be a multiple of.
        workgroup_size: u32,
    },
    /// `elements` is over [`MAX_ELEMENTS`].
    TooManyElements {
        /// What was asked for.
        elements: u32,
        /// The ceiling.
        max: u32,
    },
    /// `dispatches` was zero.
    ZeroDispatches,
    /// `dispatches` is over [`MAX_DISPATCHES`].
    TooManyDispatches {
        /// What was asked for.
        dispatches: u32,
        /// The ceiling.
        max: u32,
    },
}

impl std::fmt::Display for SpecError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ZeroElements => f.write_str("element count is zero"),
            Self::NotMultipleOfWorkgroup {
                elements,
                workgroup_size,
            } => write!(
                f,
                "element count {elements} is not a multiple of the workgroup size {workgroup_size}"
            ),
            Self::TooManyElements { elements, max } => {
                write!(f, "element count {elements} exceeds the ceiling of {max}")
            }
            Self::ZeroDispatches => f.write_str("dispatch count is zero"),
            Self::TooManyDispatches { dispatches, max } => {
                write!(
                    f,
                    "dispatch count {dispatches} exceeds the ceiling of {max}"
                )
            }
        }
    }
}

impl std::error::Error for SpecError {}

impl Spec {
    /// A spec with the default element count, one dispatch, and `seed`.
    #[must_use]
    pub const fn new(seed: u64) -> Self {
        Self {
            elements: DEFAULT_ELEMENTS,
            dispatches: 1,
            seed,
        }
    }

    /// Sets the element count.
    #[must_use]
    pub const fn with_elements(mut self, elements: u32) -> Self {
        self.elements = elements;
        self
    }

    /// Sets the dispatch count.
    #[must_use]
    pub const fn with_dispatches(mut self, dispatches: u32) -> Self {
        self.dispatches = dispatches;
        self
    }

    /// Checks that this spec can be dispatched.
    ///
    /// # Errors
    ///
    /// Returns a [`SpecError`] naming the field and both the value and the
    /// limit it violated.
    pub const fn validate(&self) -> Result<(), SpecError> {
        if self.elements == 0 {
            return Err(SpecError::ZeroElements);
        }
        if !self.elements.is_multiple_of(WORKGROUP_SIZE) {
            return Err(SpecError::NotMultipleOfWorkgroup {
                elements: self.elements,
                workgroup_size: WORKGROUP_SIZE,
            });
        }
        if self.elements > MAX_ELEMENTS {
            return Err(SpecError::TooManyElements {
                elements: self.elements,
                max: MAX_ELEMENTS,
            });
        }
        if self.dispatches == 0 {
            return Err(SpecError::ZeroDispatches);
        }
        if self.dispatches > MAX_DISPATCHES {
            return Err(SpecError::TooManyDispatches {
                dispatches: self.dispatches,
                max: MAX_DISPATCHES,
            });
        }
        Ok(())
    }

    /// Workgroups one dispatch launches.
    ///
    /// Exact, because [`Spec::validate`] has already required `elements` to
    /// be a multiple of [`WORKGROUP_SIZE`].
    #[must_use]
    pub const fn workgroups(&self) -> u32 {
        self.elements / WORKGROUP_SIZE
    }

    /// Bytes in one buffer.
    #[must_use]
    pub fn buffer_bytes(&self) -> u64 {
        u64::from(self.elements) * ELEMENT_BYTES
    }

    /// The two operand buffers.
    ///
    /// Derived from `seed`, so the same spec always produces the same work.
    /// Every value is a canonical M31 element; see the module docs for why
    /// the values are pseudorandom rather than a ramp or a constant.
    #[must_use]
    pub fn inputs(&self) -> (Vec<u32>, Vec<u32>) {
        let count = self.elements as usize;
        let mut lhs = Vec::with_capacity(count);
        let mut rhs = Vec::with_capacity(count);
        let mut state = self.seed;

        for _ in 0..count {
            lhs.push(m31::from_u64(splitmix64(&mut state)));
            rhs.push(m31::from_u64(splitmix64(&mut state)));
        }
        (lhs, rhs)
    }

    /// The product of the two operand buffers, computed on the CPU.
    ///
    /// This is what the GPU's output is compared against. It is derived from
    /// [`Spec::inputs`] rather than being passed in, so the reference and the
    /// device under test cannot be handed different operands.
    #[must_use]
    pub fn expected(&self) -> Vec<u32> {
        let (lhs, rhs) = self.inputs();
        m31::mul_all(&lhs, &rhs)
    }

    /// The `workload_id` this run reports.
    ///
    /// The element count and the dispatch count are in the id rather than in
    /// fields of their own because the schema has nowhere to put them: a
    /// report carries one `system` and one `workload_id`, and `system` is
    /// reserved for what the workload *is*, not how big it was.
    #[must_use]
    pub fn workload_id(&self) -> String {
        format!("webgpu-m31-mul-{}-x{}", self.elements, self.dispatches)
    }
}

/// Encodes field elements into the bytes a storage buffer wants.
///
/// This function is the host↔device wire format, and it is spelled out
/// rather than reached through a `bytemuck`-style cast for two reasons.
/// First, `unsafe_code` is forbidden workspace-wide, and a reinterpret cast
/// is the one thing that would need it. Second, the endianness is the sort of
/// detail that is invisible on every little-endian host and then wrong
/// somewhere else; naming it here means the format is a decision on record.
///
/// WebGPU storage buffers of `array<u32>` are little-endian, four bytes an
/// element, tightly packed.
#[must_use]
pub fn elements_to_le_bytes(values: &[u32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(values.len() * 4);
    for value in values {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes
}

/// Decodes a storage buffer back into field elements.
///
/// Returns `None` when the byte count is not a multiple of four, which would
/// mean the device handed back a buffer shaped differently from the one that
/// was uploaded. That is a plumbing failure and the caller reports it as one
/// rather than truncating.
#[must_use]
pub fn elements_from_le_bytes(bytes: &[u8]) -> Option<Vec<u32>> {
    let (elements, remainder) = bytes.as_chunks::<4>();
    if !remainder.is_empty() {
        return None;
    }
    Some(
        elements
            .iter()
            .map(|chunk| u32::from_le_bytes(*chunk))
            .collect(),
    )
}

/// The knobs a page can set, together with what this crate defaults to and
/// the ceilings it refuses.
///
/// Sent to the page so the inputs it renders and the values the crate
/// accepts cannot disagree. A page that hard-coded its own copy of
/// `WORKGROUP_SIZE` would eventually offer an element count the run rejects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Limits {
    /// Default element count.
    pub elements: u32,
    /// Default dispatch count.
    pub dispatches: u32,
    /// [`MAX_ELEMENTS`].
    pub max_elements: u32,
    /// [`MAX_DISPATCHES`].
    pub max_dispatches: u32,
    /// [`WORKGROUP_SIZE`], which element counts must be a multiple of.
    pub workgroup_size: u32,
    /// [`ELEMENT_BYTES`].
    pub element_bytes: u64,
    /// [`DEFAULT_SEED`].
    pub seed: u64,
}

/// The limits a page should build its inputs from.
#[must_use]
pub fn limits() -> Limits {
    let default = Spec::default();
    Limits {
        elements: default.elements,
        dispatches: default.dispatches,
        max_elements: MAX_ELEMENTS,
        max_dispatches: MAX_DISPATCHES,
        workgroup_size: WORKGROUP_SIZE,
        element_bytes: ELEMENT_BYTES,
        seed: default.seed,
    }
}

/// The `system` every report from this crate carries.
///
/// `proof_system` is `"none"` because there is no proof here. The schema
/// requires the field and a `minLength` of 1, so something has to go in it,
/// and the alternatives all overclaim: naming a proof system would describe
/// work that did not happen, and an empty string is rejected. `none` is the
/// honest answer to a question the schema insists on asking.
///
/// `curve_or_field` is `"m31"`, which is simply true.
#[must_use]
pub fn system() -> zkprof_core::System {
    zkprof_core::System::new("none", "m31")
}

/// One 64-bit step of splitmix64.
///
/// Chosen over a crate dependency because the only thing needed from it is a
/// deterministic, well-distributed stream, and a generator that changes with
/// a `cargo update` would silently invalidate every stored comparison.
fn splitmix64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

#[cfg(test)]
mod tests {
    use super::{
        elements_from_le_bytes, elements_to_le_bytes, splitmix64, system, Spec, SpecError,
        DEFAULT_ELEMENTS, DEFAULT_SEED, MAX_DISPATCHES, MAX_ELEMENTS, WORKGROUP_SIZE,
    };
    use crate::m31::{self, M31_MAX};

    #[test]
    fn the_default_spec_validates() {
        Spec::default()
            .validate()
            .expect("the default must be runnable");
    }

    #[test]
    fn the_default_element_count_is_a_whole_number_of_workgroups() {
        assert_eq!(DEFAULT_ELEMENTS % WORKGROUP_SIZE, 0);
        assert_eq!(
            Spec::default().workgroups(),
            DEFAULT_ELEMENTS / WORKGROUP_SIZE
        );
    }

    #[test]
    fn a_zero_element_count_is_refused() {
        let spec = Spec::default().with_elements(0);
        assert_eq!(spec.validate(), Err(SpecError::ZeroElements));
    }

    #[test]
    fn a_partial_workgroup_is_refused() {
        let spec = Spec::default().with_elements(WORKGROUP_SIZE + 1);
        assert_eq!(
            spec.validate(),
            Err(SpecError::NotMultipleOfWorkgroup {
                elements: WORKGROUP_SIZE + 1,
                workgroup_size: WORKGROUP_SIZE,
            })
        );
    }

    #[test]
    fn an_element_count_over_the_ceiling_is_refused() {
        let spec = Spec::default().with_elements(MAX_ELEMENTS + WORKGROUP_SIZE);
        assert!(matches!(
            spec.validate(),
            Err(SpecError::TooManyElements { .. })
        ));
    }

    #[test]
    fn zero_dispatches_is_refused() {
        let spec = Spec::default().with_dispatches(0);
        assert_eq!(spec.validate(), Err(SpecError::ZeroDispatches));
    }

    #[test]
    fn more_than_the_dispatch_ceiling_is_refused() {
        let spec = Spec::default().with_dispatches(MAX_DISPATCHES + 1);
        assert!(matches!(
            spec.validate(),
            Err(SpecError::TooManyDispatches { .. })
        ));
    }

    #[test]
    fn every_refusal_names_a_number_a_reader_can_act_on() {
        let message = SpecError::NotMultipleOfWorkgroup {
            elements: 100,
            workgroup_size: 64,
        }
        .to_string();
        assert!(
            message.contains("100") && message.contains("64"),
            "{message}"
        );

        let message = SpecError::TooManyElements {
            elements: MAX_ELEMENTS + 1,
            max: MAX_ELEMENTS,
        }
        .to_string();
        assert!(message.contains("16777217"), "{message}");
    }

    #[test]
    fn the_same_spec_always_produces_the_same_operands() {
        let a = Spec::default().inputs();
        let b = Spec::default().inputs();
        assert_eq!(a, b);

        let other = Spec::new(DEFAULT_SEED + 1).inputs();
        assert_ne!(a, other, "a different seed must give different work");
    }

    #[test]
    fn operands_are_canonical_field_elements() {
        let (lhs, rhs) = Spec::default().with_elements(WORKGROUP_SIZE).inputs();
        assert_eq!(lhs.len(), WORKGROUP_SIZE as usize);
        assert_eq!(rhs.len(), WORKGROUP_SIZE as usize);
        for value in lhs.iter().chain(rhs.iter()) {
            assert!(*value <= M31_MAX, "{value} is not canonical");
        }
    }

    #[test]
    fn operands_are_not_a_constant_or_a_ramp() {
        // The point of pseudorandom operands: a kernel that only handles one
        // narrow range correctly must not pass.
        let (lhs, rhs) = Spec::default().with_elements(WORKGROUP_SIZE).inputs();
        assert!(lhs.iter().any(|value| *value != lhs[0]));
        assert!(rhs.iter().any(|value| *value != rhs[0]));
        assert_ne!(lhs, rhs);
    }

    #[test]
    fn expected_is_the_elementwise_product_of_the_operands() {
        let spec = Spec::default().with_elements(WORKGROUP_SIZE);
        let (lhs, rhs) = spec.inputs();
        let expected = spec.expected();

        assert_eq!(expected.len(), lhs.len());
        for (index, value) in expected.iter().enumerate() {
            assert_eq!(*value, m31::mul(lhs[index], rhs[index]), "at {index}");
            assert_eq!(*value, m31::mul_reference(lhs[index], rhs[index]));
        }
    }

    #[test]
    fn the_workload_id_carries_both_knobs() {
        let spec = Spec::default().with_elements(1024).with_dispatches(8);
        assert_eq!(spec.workload_id(), "webgpu-m31-mul-1024-x8");
    }

    #[test]
    fn buffer_bytes_is_elements_times_four() {
        let spec = Spec::default().with_elements(1024);
        assert_eq!(spec.buffer_bytes(), 4096);
    }

    #[test]
    fn the_system_says_there_is_no_proof_system() {
        let system = system();
        assert_eq!(system.proof_system, "none");
        assert_eq!(system.curve_or_field, "m31");
        assert!(
            system.constraint_count.is_none(),
            "a kernel has no constraints; the field must be absent, not 0"
        );
    }

    #[test]
    fn the_generator_is_deterministic() {
        // Pinned so a change to the constants above is visible rather than
        // silently changing every operand buffer in the repository.
        let mut state = 0u64;
        let first = splitmix64(&mut state);
        let second = splitmix64(&mut state);

        let mut again = 0u64;
        assert_eq!(first, splitmix64(&mut again));
        assert_eq!(second, splitmix64(&mut again));
        assert_ne!(first, second);
    }

    #[test]
    fn elements_round_trip_through_the_wire_format() {
        let values = [0u32, 1, 0xFF, 0x1_0000, M31_MAX];
        let bytes = elements_to_le_bytes(&values);
        assert_eq!(bytes.len(), values.len() * 4);
        assert_eq!(elements_from_le_bytes(&bytes), Some(values.to_vec()));
    }

    #[test]
    fn the_limits_a_page_renders_from_are_the_ones_the_crate_enforces() {
        let limits = super::limits();

        // A page that offers exactly these must not have the run refused.
        Spec::default()
            .with_elements(limits.elements)
            .with_dispatches(limits.dispatches)
            .validate()
            .expect("the advertised defaults must validate");

        assert_eq!(limits.workgroup_size, WORKGROUP_SIZE);
        assert_eq!(limits.element_bytes, 4);
        assert!(limits.max_elements >= limits.elements);
        assert!(limits.max_dispatches >= limits.dispatches);
    }

    #[test]
    fn the_wire_format_is_little_endian() {
        // Pinned because the layout is a claim about what the device sees,
        // and a big-endian host that produced something else would be a
        // silent disagreement with the shader rather than a loud failure.
        assert_eq!(elements_to_le_bytes(&[0x0102_0304]), vec![4, 3, 2, 1]);
    }

    #[test]
    fn a_buffer_that_is_not_a_whole_number_of_elements_is_refused() {
        assert_eq!(elements_from_le_bytes(&[0, 0, 0, 0, 0]), None);
        assert_eq!(elements_from_le_bytes(&[]), Some(vec![]));
    }
}
