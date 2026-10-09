// SPDX-License-Identifier: Apache-2.0

//! A depth-parameterised Merkle-Poseidon membership circuit.
//!
//! # Why this circuit
//!
//! The W3.2 gate needs a *real* circuit, not a synthetic constraint
//! generator: an honest split between witness generation, MSM and FFT only
//! means something if the workload is one that a real system would run. A
//! Merkle membership proof over a Poseidon hash is the shape that dominates
//! production recursion and rollup circuits, and its cost is dominated by
//! two things this harness is specifically built to separate — Poseidon
//! permutations (witness generation) and multi-scalar multiplication (the
//! Groth16 proof itself).
//!
//! # Sizing
//!
//! `depth` is the number of tree levels and the constraint count is exactly
//! `243 * depth + 1`, measured rather than derived:
//!
//! | per level | constraints | why |
//! |---|---|---|
//! | Poseidon permutation | 240 | 81 S-boxes at 3 constraints each, less the 3 the first full round spends on an all-constant state |
//! | position bit | 1 | `Boolean::new_witness` enforces `b * (1 - b) = 0` |
//! | two conditional selects | 2 | one each, ordering the pair for the hash |
//! | per circuit | 1 | the root equality |
//!
//! [`DEFAULT_DEPTH`] is 64, which is 15,553 constraints — within 5% of 2^14.
//!
//! # The `hash` primitive is not used here
//!
//! It would be reasonable to expect Poseidon permutations to be attributed
//! to `hash`. They are not: they run inside `generate_constraints`, which is
//! one indivisible call into arkworks with no hook to subdivide it.
//! Splitting them out would mean patching upstream. They are therefore
//! attributed to `witness_gen`, which is what the call as a whole computes.

use ark_crypto_primitives::sponge::constraints::CryptographicSpongeVar;
use ark_crypto_primitives::sponge::poseidon::constraints::PoseidonSpongeVar;
use ark_crypto_primitives::sponge::poseidon::{find_poseidon_ark_and_mds, PoseidonConfig};
use ark_crypto_primitives::sponge::{Absorb, CryptographicSponge, FieldBasedCryptographicSponge};
use ark_ff::PrimeField;
use ark_r1cs_std::alloc::AllocVar;
use ark_r1cs_std::fields::fp::FpVar;
use ark_r1cs_std::prelude::{Boolean, CondSelectGadget, EqGadget};
use ark_relations::r1cs::{ConstraintSynthesizer, ConstraintSystemRef, SynthesisError};
use ark_std::rand::Rng;

/// Sponge rate: two field elements absorbed per permutation.
pub const RATE: usize = 2;

/// Sponge capacity, in field elements. Fixes the state at `RATE + CAPACITY`.
pub const CAPACITY: usize = 1;

/// Poseidon full rounds. With `PARTIAL_ROUNDS` below, the 128-bit secure
/// parameter set for a state of three BN254 field elements.
pub const FULL_ROUNDS: usize = 8;

/// Poseidon partial rounds.
pub const PARTIAL_ROUNDS: usize = 57;

/// S-box exponent.
pub const ALPHA: u64 = 5;

/// Tree depth that puts the default circuit near 2^14 constraints.
///
/// The circuit has `243 * depth + 1` constraints (see the module docs for
/// where 243 comes from), so this is 15,553. The count is also measured at
/// runtime and recorded in each report as `system.constraint_count`; nothing
/// in this crate predicts it, it just checks the two agree.
pub const DEFAULT_DEPTH: usize = 64;

/// Constraints added by each tree level.
///
/// The constant in `243 * depth + 1`. Kept here so the tests, the docs and
/// the README have one source for it.
pub const CONSTRAINTS_PER_LEVEL: usize = 243;

/// Constraints independent of depth: the root equality.
pub const CONSTRAINTS_FIXED: usize = 1;

/// Sponge parameters shared by the native witness builder and the circuit.
///
/// Generated deterministically from the Poseidon Grain LFSR for
/// `F::MODULUS_BIT_SIZE`. Not read from a file and not configurable: the
/// point of the default is that two runs of this crate produce the same
/// circuit, so their reports are comparable.
///
/// # Panics
///
/// Panics if arkworks' parameter generator rejects the field, which it does
/// not for BN254's scalar field.
#[must_use]
pub fn poseidon_config<F: PrimeField>() -> PoseidonConfig<F> {
    let (ark, mds) = find_poseidon_ark_and_mds::<F>(
        u64::from(F::MODULUS_BIT_SIZE),
        RATE,
        FULL_ROUNDS as u64,
        PARTIAL_ROUNDS as u64,
        0,
    );
    PoseidonConfig::new(FULL_ROUNDS, PARTIAL_ROUNDS, ALPHA, mds, ark, RATE, CAPACITY)
}

/// Native 2:1 Poseidon compression, mirroring the in-circuit sponge.
///
/// This must duplicate the gadget's absorb/squeeze schedule exactly, or the
/// witness this builds will not satisfy the circuit. Both sides absorb two
/// elements and then squeeze one, which produces exactly one permutation on
/// each. `tests::native_compression_matches_the_circuit` checks the two
/// agree.
#[must_use]
pub fn compress<F: PrimeField + Absorb>(params: &PoseidonConfig<F>, left: F, right: F) -> F {
    let mut sponge = ark_crypto_primitives::sponge::poseidon::PoseidonSponge::new(params);
    sponge.absorb(&vec![left, right]);
    sponge.squeeze_native_field_elements(1)[0]
}

/// Proof of membership of `leaf` in a depth-`depth` Poseidon Merkle tree.
///
/// The tree is not materialised: only the authentication path is needed to
/// build the witness, and the path determines the root, so an instance of
/// arbitrary depth costs `depth` hashes to construct rather than `2^depth`.
#[derive(Clone)]
pub struct MerklePoseidonCircuit<F: PrimeField> {
    /// Sponge parameters, shared by every level.
    pub params: PoseidonConfig<F>,

    /// Number of hash levels between the leaf and the root.
    pub depth: usize,

    /// The claimed root. A public input; `None` during key generation.
    pub root: Option<F>,

    /// The leaf being proven. A witness; `None` during key generation.
    pub leaf: Option<F>,

    /// One sibling hash per level, leaf-to-root. `None` during key generation.
    pub siblings: Vec<Option<F>>,

    /// One position bit per level. `true` puts the sibling on the left.
    /// `None` during key generation.
    pub positions: Vec<Option<bool>>,
}

impl<F: PrimeField> MerklePoseidonCircuit<F> {
    /// A circuit with no assignment, for key generation.
    ///
    /// Synthesis over a blank circuit allocates the same variables and
    /// enforces the same constraints; only the field values are missing.
    #[must_use]
    pub fn blank(params: PoseidonConfig<F>, depth: usize) -> Self {
        Self {
            params,
            depth,
            root: None,
            leaf: None,
            siblings: vec![None; depth],
            positions: vec![None; depth],
        }
    }

    /// Attaches a full membership witness.
    ///
    /// # Panics
    ///
    /// Panics if `siblings` or `positions` is not exactly `depth` long.
    #[must_use]
    pub fn with_witness(
        mut self,
        root: F,
        leaf: F,
        siblings: Vec<F>,
        positions: Vec<bool>,
    ) -> Self {
        assert_eq!(siblings.len(), self.depth, "need one sibling per level");
        assert_eq!(positions.len(), self.depth, "need one position per level");
        self.root = Some(root);
        self.leaf = Some(leaf);
        self.siblings = siblings.into_iter().map(Some).collect();
        self.positions = positions.into_iter().map(Some).collect();
        self
    }
}

impl<F: PrimeField> ConstraintSynthesizer<F> for MerklePoseidonCircuit<F> {
    fn generate_constraints(self, cs: ConstraintSystemRef<F>) -> Result<(), SynthesisError> {
        let missing = || SynthesisError::AssignmentMissing;

        let root = FpVar::new_input(cs.clone(), || self.root.ok_or_else(missing))?;
        let mut cur = FpVar::new_witness(cs.clone(), || self.leaf.ok_or_else(missing))?;

        let mut siblings = Vec::with_capacity(self.depth);
        let mut positions = Vec::with_capacity(self.depth);
        for level in 0..self.depth {
            siblings.push(FpVar::new_witness(cs.clone(), || {
                self.siblings[level].ok_or_else(missing)
            })?);
            positions.push(Boolean::new_witness(cs.clone(), || {
                self.positions[level].ok_or_else(missing)
            })?);
        }

        for level in 0..self.depth {
            let sibling = &siblings[level];
            // `positions[level] == true` means the sibling hashes on the left.
            let left = FpVar::conditionally_select(&positions[level], sibling, &cur)?;
            let right = FpVar::conditionally_select(&positions[level], &cur, sibling)?;

            let mut sponge = PoseidonSpongeVar::new(cs.clone(), &self.params);
            sponge.absorb(&vec![left, right])?;
            cur = sponge.squeeze_field_elements(1)?.remove(0);
        }

        cur.enforce_equal(&root)
    }
}

/// Builds a membership witness for an arbitrary leaf.
///
/// The tree itself is never constructed. Siblings and position bits are
/// drawn from `rng` and the root is whatever the resulting path hashes to,
/// which is a perfectly good tree for profiling: the prover's work depends
/// on the depth and the field, not on which leaf was chosen.
///
/// Returns `(root, siblings, positions)` in the shape
/// [`MerklePoseidonCircuit::with_witness`] expects.
pub fn membership_witness<F: PrimeField + Absorb, R: Rng>(
    params: &PoseidonConfig<F>,
    leaf: F,
    depth: usize,
    rng: &mut R,
) -> (F, Vec<F>, Vec<bool>) {
    let mut siblings = Vec::with_capacity(depth);
    let mut positions = Vec::with_capacity(depth);
    for _ in 0..depth {
        siblings.push(F::rand(rng));
        let position: bool = rng.gen();
        positions.push(position);
    }

    let mut cur = leaf;
    for level in 0..depth {
        cur = if positions[level] {
            compress(params, siblings[level], cur)
        } else {
            compress(params, cur, siblings[level])
        };
    }

    (cur, siblings, positions)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ark_bn254::Fr;
    use ark_relations::r1cs::ConstraintSystem;
    use ark_std::rand::rngs::StdRng;
    use ark_std::rand::SeedableRng;

    fn rng() -> StdRng {
        StdRng::seed_from_u64(0x5EED)
    }

    #[test]
    fn native_compression_matches_the_circuit() {
        let params = poseidon_config::<Fr>();
        let (left, right) = (Fr::from(7u8), Fr::from(11u8));

        let cs = ConstraintSystem::<Fr>::new_ref();
        let circuit = MerklePoseidonCircuit::blank(params.clone(), 1).with_witness(
            compress(&params, left, right),
            left,
            vec![right],
            vec![false],
        );
        circuit.generate_constraints(cs.clone()).unwrap();

        assert!(
            cs.is_satisfied().unwrap(),
            "the native witness builder and the in-circuit sponge disagree"
        );
    }

    #[test]
    fn a_witness_built_for_the_right_root_satisfies_the_circuit() {
        let params = poseidon_config::<Fr>();
        let mut rng = rng();
        let leaf = Fr::from(3u8);
        let (root, siblings, positions) = membership_witness(&params, leaf, 8, &mut rng);

        let cs = ConstraintSystem::<Fr>::new_ref();
        MerklePoseidonCircuit::blank(params, 8)
            .with_witness(root, leaf, siblings, positions)
            .generate_constraints(cs.clone())
            .unwrap();

        assert!(cs.is_satisfied().unwrap());
        assert_eq!(
            cs.num_instance_variables(),
            2,
            "one public input, plus the constant one"
        );
        assert_eq!(
            cs.num_constraints(),
            8 * CONSTRAINTS_PER_LEVEL + CONSTRAINTS_FIXED
        );
    }

    /// The `243 * depth + 1` model is a claim about the circuit, so it gets
    /// checked at several depths rather than asserted in a comment.
    #[test]
    fn the_constraint_model_holds_across_depths() {
        for depth in [1_usize, 2, 5, 16, 64] {
            let params = poseidon_config::<Fr>();
            let mut rng = rng();
            let leaf = Fr::from(1u8);
            let (root, siblings, positions) = membership_witness(&params, leaf, depth, &mut rng);

            let cs = ConstraintSystem::<Fr>::new_ref();
            MerklePoseidonCircuit::blank(params, depth)
                .with_witness(root, leaf, siblings, positions)
                .generate_constraints(cs.clone())
                .unwrap();

            assert_eq!(
                cs.num_constraints(),
                depth * CONSTRAINTS_PER_LEVEL + CONSTRAINTS_FIXED,
                "depth {depth}"
            );
        }
    }

    #[test]
    fn a_wrong_root_is_rejected() {
        let params = poseidon_config::<Fr>();
        let mut rng = rng();
        let leaf = Fr::from(3u8);
        let (root, siblings, positions) = membership_witness(&params, leaf, 8, &mut rng);

        let cs = ConstraintSystem::<Fr>::new_ref();
        MerklePoseidonCircuit::blank(params, 8)
            .with_witness(root + Fr::from(1u8), leaf, siblings, positions)
            .generate_constraints(cs.clone())
            .unwrap();

        assert!(!cs.is_satisfied().unwrap());
    }

    #[test]
    fn the_default_depth_lands_near_two_pow_fourteen() {
        let params = poseidon_config::<Fr>();
        let mut rng = rng();
        let leaf = Fr::from(1u8);
        let (root, siblings, positions) =
            membership_witness(&params, leaf, DEFAULT_DEPTH, &mut rng);

        let cs = ConstraintSystem::<Fr>::new_ref();
        MerklePoseidonCircuit::blank(params, DEFAULT_DEPTH)
            .with_witness(root, leaf, siblings, positions)
            .generate_constraints(cs.clone())
            .unwrap();

        let count = cs.num_constraints();
        let target = 1_usize << 14;
        // Integer cross-multiplication rather than a float ratio: there is
        // no precision question, and 0.9..=1.1 becomes an exact band.
        assert!(
            count * 10 >= target * 9 && count * 10 <= target * 11,
            "default depth gave {count} constraints, which is outside +/-10% of 2^14 = {target}"
        );
    }

    #[test]
    fn compression_is_order_dependent() {
        let params = poseidon_config::<Fr>();
        let (a, b) = (Fr::from(2u8), Fr::from(5u8));
        assert_ne!(compress(&params, a, b), compress(&params, b, a));
    }
}
