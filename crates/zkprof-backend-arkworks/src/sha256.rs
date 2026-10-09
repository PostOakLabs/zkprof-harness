// SPDX-License-Identifier: Apache-2.0

//! A SHA-256 preimage circuit, sized to the csp-benchmarks canonical inputs.
//!
//! # Why this circuit
//!
//! csp-benchmarks measures a fixed set of canonical circuits — `sha256`,
//! `keccak`, `ecdsa`, `blake3`, `poseidon`, `poseidon2` — and publishes
//! results quarterly on ethproofs.org. Its `sha256` target is "prove you know
//! a preimage of this digest", parameterised by input size in bytes.
//!
//! The Merkle-Poseidon circuit in [`crate::circuit`] is the right shape for
//! this harness, but it is not a circuit anyone else benchmarks, so a number
//! measured on it cannot be compared with anything outside this repository.
//! This one can be. Running the same circuit natively and in a browser is the
//! tool's thesis in a single row of a table, and it is only a comparable row
//! if the circuit is one the field already measures.
//!
//! # What is being proven
//!
//! Knowledge of an `input_bytes`-byte preimage of a public 32-byte digest. The
//! digest is a public input and the preimage is a witness. That is the
//! statement csp-benchmarks' `sha256` target measures — not an approximation
//! of it, and not a SHA-256 *compression function* benchmark, which is a
//! different target with a different cost model.
//!
//! # Sizing
//!
//! [`SHA256_INPUT_SIZES`] is csp-benchmarks' own byte-size list, imported from
//! [`zkprof_core::csp::BYTE_INPUTS_FULL`] rather than restated, so the
//! workload cannot drift from the sizes the exporter claims to support.
//!
//! The constraint count is measured at runtime and recorded in each report.
//! Nothing in this crate predicts it: SHA-256's cost per 64-byte block is a
//! property of the upstream gadget, and a number this crate invented would be
//! worse than no number.
//!
//! # Public-input encoding, and why key generation uses zeros
//!
//! The digest enters the circuit through [`UInt8::new_input_vec`], which packs
//! bytes into field elements, allocates those as public inputs, and unpacks
//! them back into bytes in-circuit. That costs constraints — a bit
//! decomposition per element — which upstream documents as a deliberate trade
//! for a smaller public-input vector.
//!
//! The consequence that matters here: `new_input_vec` takes *values*, not a
//! closure, so it cannot report a missing assignment the way
//! [`FpVar::new_input`] does. Key generation therefore supplies 32 zero bytes.
//! The circuit's shape depends only on the length of the slice and not on its
//! contents, so this produces the same key as any other digest would — but
//! that is a claim about the shape, so
//! `tests::a_blank_circuit_has_the_shape_of_a_witnessed_one` checks it rather
//! than assuming it.
//!
//! # A blank circuit only synthesises in `Setup` mode
//!
//! This is the part that is easy to get wrong, and it is not visible from the
//! API. `ark_relations` gives
//! [`ConstraintSystem::new_ref`](ark_relations::r1cs::ConstraintSystem::new_ref)
//! a **`Prove` default**, in which every witness closure is invoked.
//! Synthesising a blank circuit there fails with `AssignmentMissing` at the
//! first preimage byte — which is correct behaviour, because in `Prove` mode
//! there is genuinely no value to record.
//!
//! Key generation is therefore the only context `Self::blank` is for, and it
//! runs in `Setup`, where closures are skipped and no assignment is ever asked
//! for. Groth16's setup does this itself; the tests have to do it by hand,
//! which is why `tests::synthesize_setup` exists next to `tests::synthesize`
//! and why the two are not interchangeable.
//!
//! The shape is identical in both modes. That is what
//! `tests::a_blank_circuit_has_the_shape_of_a_witnessed_one` actually pins
//! down — not that the numbers agree by luck, but that a key generated from a
//! blank circuit is a key for the circuit that later proves.

use ark_crypto_primitives::crh::sha256::constraints::Sha256Gadget;
use ark_crypto_primitives::crh::sha256::{digest::Digest, Sha256};
use ark_ff::PrimeField;
use ark_r1cs_std::alloc::AllocVar;
use ark_r1cs_std::prelude::EqGadget;
use ark_r1cs_std::uint8::UInt8;
use ark_relations::r1cs::{ConstraintSynthesizer, ConstraintSystemRef, SynthesisError};
use zkprof_core::csp::BYTE_INPUTS_FULL;

/// Length of a SHA-256 digest in bytes.
pub const DIGEST_BYTES: usize = 32;

/// The input sizes csp-benchmarks measures for byte-oriented circuits.
///
/// Deliberately the same array the exporter advertises
/// ([`zkprof_core::csp::BYTE_INPUTS_FULL`]) and not a copy of it: a workload
/// at a size the exporter would reject is a workload this crate should not
/// offer. `u64` because that is upstream's element type; the casts at the use
/// sites are checked.
pub const SHA256_INPUT_SIZES: &[u64] = BYTE_INPUTS_FULL;

/// The default preimage length, in bytes: the smallest canonical size.
///
/// SHA-256 costs roughly one block of constraints per 64 bytes, so this is
/// also the cheapest canonical size, and the default should not be the run
/// that takes longest. It is three blocks including padding.
pub const DEFAULT_SHA256_INPUT_BYTES: usize = 128;

/// Computes SHA-256 natively, for building the witness.
///
/// This is the plain RustCrypto hash, not a gadget: it runs outside the
/// circuit and its result is what the circuit's public input is set to. If the
/// two ever disagreed, every proof would fail, and
/// `tests::the_native_digest_matches_a_known_vector` pins the native side to
/// a published test vector so the disagreement could not be blamed on the
/// circuit.
#[must_use]
pub fn sha256_digest(preimage: &[u8]) -> [u8; DIGEST_BYTES] {
    let mut digest = [0u8; DIGEST_BYTES];
    digest.copy_from_slice(&Sha256::digest(preimage));
    digest
}

/// Proof of knowledge of a preimage of a SHA-256 digest.
///
/// Not generic over the field: nothing in the struct holds a field element,
/// and [`ConstraintSynthesizer`] is implemented for every field below. A
/// `PhantomData` would only exist to satisfy a bound the struct does not
/// need.
#[derive(Clone)]
pub struct Sha256PreimageCircuit {
    /// Preimage length in bytes. This is the workload: two circuits of
    /// different lengths are not measurements of the same thing.
    pub input_bytes: usize,

    /// The claimed digest. A public input.
    pub digest: Option<[u8; DIGEST_BYTES]>,

    /// The preimage. A witness.
    pub preimage: Option<Vec<u8>>,
}

impl Sha256PreimageCircuit {
    /// A circuit with no assignment, for key generation.
    ///
    /// See the module docs on why "no assignment" still means 32 zero bytes
    /// for the digest: [`UInt8::new_input_vec`] needs values, and the shape —
    /// which is all setup depends on — is unaffected by what they are.
    #[must_use]
    pub fn blank(input_bytes: usize) -> Self {
        Self {
            input_bytes,
            digest: None,
            preimage: None,
        }
    }

    /// Attaches the digest and its preimage.
    ///
    /// # Panics
    ///
    /// Panics if `preimage.len()` is not exactly `input_bytes`. A shorter
    /// preimage is a different workload, not a padded one.
    #[must_use]
    pub fn with_witness(mut self, digest: [u8; DIGEST_BYTES], preimage: Vec<u8>) -> Self {
        assert_eq!(
            preimage.len(),
            self.input_bytes,
            "preimage must be exactly {} bytes",
            self.input_bytes
        );
        self.digest = Some(digest);
        self.preimage = Some(preimage);
        self
    }
}

impl<F: PrimeField> ConstraintSynthesizer<F> for Sha256PreimageCircuit {
    fn generate_constraints(self, cs: ConstraintSystemRef<F>) -> Result<(), SynthesisError> {
        let missing = || SynthesisError::AssignmentMissing;

        let digest = self.digest.unwrap_or([0u8; DIGEST_BYTES]);
        let expected = UInt8::new_input_vec(cs.clone(), &digest)?;

        let mut preimage = Vec::with_capacity(self.input_bytes);
        for index in 0..self.input_bytes {
            preimage.push(UInt8::new_witness(cs.clone(), || {
                self.preimage
                    .as_ref()
                    .map(|bytes| bytes[index])
                    .ok_or_else(missing)
            })?);
        }

        let actual = Sha256Gadget::<F>::digest(&preimage)?;
        expected.enforce_equal(&actual.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ark_bn254::Fr;
    use ark_relations::r1cs::{ConstraintSystem, SynthesisMode};

    /// A deterministic preimage. Any bytes would do; these are fixed so a
    /// failure is reproducible.
    fn preimage(len: usize) -> Vec<u8> {
        (0..len).map(|i| u8::try_from(i % 251).unwrap()).collect()
    }

    /// Synthesis in the default mode, which is `Prove`: witness closures are
    /// evaluated and their values recorded, so `is_satisfied()` afterwards
    /// means something.
    fn synthesize(
        circuit: Sha256PreimageCircuit,
    ) -> (ConstraintSystemRef<Fr>, Result<(), SynthesisError>) {
        let cs = ConstraintSystem::<Fr>::new_ref();
        let result = circuit.generate_constraints(cs.clone());
        (cs, result)
    }

    /// Synthesis in `Setup` mode — the mode Groth16 key generation actually
    /// uses, and the only mode in which a [`Sha256PreimageCircuit::blank`]
    /// circuit means anything.
    ///
    /// The difference is not cosmetic. `ark_relations` skips witness closures
    /// in `Setup` and never fills in `witness_assignment`, which is precisely
    /// why a blank circuit can be synthesised here and not in `Prove`: there
    /// is no assignment to be missing, because none is asked for. Synthesising
    /// a blank circuit in `Prove` fails with `AssignmentMissing` the moment the
    /// first preimage byte is allocated.
    fn synthesize_setup(
        circuit: Sha256PreimageCircuit,
    ) -> (ConstraintSystemRef<Fr>, Result<(), SynthesisError>) {
        let cs = ConstraintSystem::<Fr>::new_ref();
        cs.set_mode(SynthesisMode::Setup);
        let result = circuit.generate_constraints(cs.clone());
        (cs, result)
    }

    /// SHA-256("abc"), as a byte array rather than a hex string so the test
    /// needs no hex crate: the point is the vector, not the encoding.
    const DIGEST_OF_ABC: [u8; DIGEST_BYTES] = [
        0xba, 0x78, 0x16, 0xbf, 0x8f, 0x01, 0xcf, 0xea, 0x41, 0x41, 0x40, 0xde, 0x5d, 0xae, 0x22,
        0x23, 0xb0, 0x03, 0x61, 0xa3, 0x96, 0x17, 0x7a, 0x9c, 0xb4, 0x10, 0xff, 0x61, 0xf2, 0x00,
        0x15, 0xad,
    ];

    #[test]
    fn the_native_digest_matches_a_known_vector() {
        assert_eq!(sha256_digest(b"abc"), DIGEST_OF_ABC);
    }

    #[test]
    fn a_witness_for_the_real_digest_satisfies_the_circuit() {
        let bytes = DEFAULT_SHA256_INPUT_BYTES;
        let preimage = preimage(bytes);
        let digest = sha256_digest(&preimage);

        let (cs, result) =
            synthesize(Sha256PreimageCircuit::blank(bytes).with_witness(digest, preimage));
        result.unwrap();
        assert!(cs.is_satisfied().unwrap());
    }

    #[test]
    fn a_wrong_digest_is_rejected() {
        let bytes = DEFAULT_SHA256_INPUT_BYTES;
        let preimage = preimage(bytes);
        let mut digest = sha256_digest(&preimage);
        digest[0] ^= 0x01;

        let (cs, result) =
            synthesize(Sha256PreimageCircuit::blank(bytes).with_witness(digest, preimage));
        result.unwrap();
        assert!(
            !cs.is_satisfied().unwrap(),
            "the circuit accepted a preimage for a digest that is not its hash"
        );
    }

    /// The load-bearing test for the zero-digest setup decision.
    ///
    /// If the shape depended on the digest bytes, the key generated from a
    /// blank circuit would not match the circuit used to prove, and Groth16
    /// would fail somewhere far away from the real cause.
    #[test]
    fn a_blank_circuit_has_the_shape_of_a_witnessed_one() {
        let bytes = DEFAULT_SHA256_INPUT_BYTES;
        let preimage = preimage(bytes);
        let digest = sha256_digest(&preimage);

        let (blank_cs, blank_result) = synthesize_setup(Sha256PreimageCircuit::blank(bytes));
        let (full_cs, full_result) =
            synthesize_setup(Sha256PreimageCircuit::blank(bytes).with_witness(digest, preimage));
        blank_result.unwrap();
        full_result.unwrap();

        assert_eq!(blank_cs.num_constraints(), full_cs.num_constraints());
        assert_eq!(
            blank_cs.num_instance_variables(),
            full_cs.num_instance_variables()
        );
        assert_eq!(
            blank_cs.num_witness_variables(),
            full_cs.num_witness_variables()
        );
    }

    /// The preimage length is the workload, so the circuit has to grow with
    /// it. Fixed-size padding would make every input size the same benchmark.
    #[test]
    fn the_circuit_grows_with_the_input_size() {
        let mut previous = 0;
        for size in SHA256_INPUT_SIZES {
            let bytes = usize::try_from(*size).unwrap();
            let (cs, result) = synthesize_setup(Sha256PreimageCircuit::blank(bytes));
            result.unwrap();

            let count = cs.num_constraints();
            assert!(
                count > previous,
                "input size {bytes} gave {count} constraints, not more than {previous}"
            );
            previous = count;
        }
    }

    #[test]
    fn the_default_input_size_is_one_csp_benchmarks_measures() {
        let default = u64::try_from(DEFAULT_SHA256_INPUT_BYTES).unwrap();
        assert!(
            SHA256_INPUT_SIZES.contains(&default),
            "{default} is not a csp-benchmarks byte input size, so the default \
             workload could not be exported: {SHA256_INPUT_SIZES:?}"
        );
    }

    /// The instance the circuit allocates is exactly the vector the benchmark
    /// verifies against.
    ///
    /// This is the test that makes `bench::digest_public_inputs` safe to call.
    /// The circuit encodes the digest by packing it into field elements and
    /// allocating those as public inputs, and the benchmark has to produce the
    /// same vector independently to verify. Two encodings that agree today and
    /// drift tomorrow would show up as a proof that mysteriously does not
    /// verify, so the agreement is asserted here instead.
    ///
    /// It reads the instance back out of the constraint system rather than
    /// recomputing it, because the point is that the two sides agree -- not
    /// that this test can do the same arithmetic twice.
    #[test]
    fn the_instance_matches_the_public_inputs_the_backend_computes() {
        let bytes = DEFAULT_SHA256_INPUT_BYTES;
        let preimage = preimage(bytes);
        let digest = sha256_digest(&preimage);

        let cs = ConstraintSystem::<Fr>::new_ref();
        Sha256PreimageCircuit::blank(bytes)
            .with_witness(digest, preimage)
            .generate_constraints(cs.clone())
            .unwrap();

        // Element 0 is the constant one every constraint system starts with;
        // the circuit's public inputs are everything after it.
        let instance: Vec<Fr> = cs.borrow().unwrap().instance_assignment[1..].to_vec();
        let computed = crate::bench::digest_public_inputs(&digest).unwrap();

        assert_eq!(
            instance, computed,
            "the circuit allocated a different instance than the benchmark verifies \
             against; a proof built here would not verify"
        );
    }
}
