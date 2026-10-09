// SPDX-License-Identifier: Apache-2.0

//! A Groth16 prover re-implemented so that each stage can be timed.
//!
//! # Why re-implement rather than wrap
//!
//! `Groth16::create_proof_with_reduction` is one opaque call: it synthesises
//! the circuit, reduces to a QAP, and assembles the proof, and it keeps its
//! own internal timers that this crate cannot read. To attribute time to
//! primitives, the call has to be taken apart.
//!
//! Every operation below is copied from `ark-groth16-0.5.0/src/prover.rs` and
//! performs arithmetic in the same order, so the [`Proof`] this produces is
//! the proof upstream would have produced. `tests::matches_the_upstream_proof`
//! asserts exactly that, and is the reason this file is allowed to exist.
//!
//! # What the spans mean, and where they are honest
//!
//! - `witness_gen` — circuit synthesis, linear-combination inlining, sparse
//!   matrix construction, and evaluation of the witness. One span because
//!   arkworks exposes one call. Note that the Poseidon permutations the
//!   Merkle circuit spends most of its time on live in here, *not* under
//!   `hash`; see the module docs in [`crate::circuit`].
//! - `ntt_fft` — the R1CS-to-QAP witness map: three inverse FFTs, two forward
//!   FFTs over a coset domain, and the polynomial multiply/divide between
//!   them. This is the only genuinely transform-shaped work in the pipeline.
//! - `field_arith` — converting witness scalars from Montgomery form into the
//!   big-integer form the MSM wants, one modular reduction per element.
//! - `msm` — the five multi-scalar multiplications: `h`, `l`, `a`, `b` in G1,
//!   and `b` in G2.
//! - `other` — group scalar multiplications by `r` and `s`, the group
//!   additions that assemble C, and the affine conversions at the end. Not
//!   "miscellaneous small stuff": it is a real cost, measured, and simply not
//!   one of the four named primitives.

use ark_ec::{pairing::Pairing, CurveGroup, VariableBaseMSM};
use ark_ff::{PrimeField, Zero};
use ark_groth16::r1cs_to_qap::R1CSToQAP;
use ark_groth16::{Proof, ProvingKey};
use ark_poly::GeneralEvaluationDomain;
use ark_relations::r1cs::{
    ConstraintSynthesizer, ConstraintSystem, OptimizationGoal, Result as R1CSResult, SynthesisError,
};
use ark_std::ops::{AddAssign, Mul};
use ark_std::vec::Vec;
use zkprof_core::model::Primitive;
use zkprof_core::profiler::Profiler;

/// The proof, and the circuit size it was produced for.
#[derive(Debug, Clone, PartialEq)]
pub struct Proved<E: Pairing> {
    /// The Groth16 proof.
    pub proof: Proof<E>,

    /// Constraints in the synthesised circuit.
    pub constraint_count: usize,
}

/// Proves `circuit`, accumulating time into `profiler` as it goes.
///
/// `r` and `s` are the prover's randomness and are passed in rather than
/// sampled so that a benchmark run is reproducible: the same circuit, key and
/// seed give the same proof and the same arithmetic.
///
/// # Errors
///
/// Propagates whatever synthesis or the QAP witness map fails with.
pub fn prove_with_spans<E, QAP, C>(
    circuit: &C,
    pk: &ProvingKey<E>,
    r: E::ScalarField,
    s: E::ScalarField,
    profiler: &Profiler<'_>,
) -> R1CSResult<Proved<E>>
where
    E: Pairing,
    QAP: R1CSToQAP,
    C: ConstraintSynthesizer<E::ScalarField> + Clone,
{
    type D<F> = GeneralEvaluationDomain<F>;

    let cs = ConstraintSystem::new_ref();
    cs.set_optimization_goal(OptimizationGoal::Constraints);

    let (matrices, full_assignment) =
        profiler.span(Primitive::WitnessGen, || -> R1CSResult<_> {
            circuit.clone().generate_constraints(cs.clone())?;
            cs.finalize();

            let matrices = cs.to_matrices().ok_or(SynthesisError::AssignmentMissing)?;
            let full_assignment = {
                let inner = cs.borrow().ok_or(SynthesisError::AssignmentMissing)?;
                let mut assignment = Vec::with_capacity(
                    inner.instance_assignment.len() + inner.witness_assignment.len(),
                );
                assignment.extend_from_slice(&inner.instance_assignment);
                assignment.extend_from_slice(&inner.witness_assignment);
                assignment
            };
            Ok((matrices, full_assignment))
        })?;

    let num_inputs = matrices.num_instance_variables;
    let num_constraints = matrices.num_constraints;
    let constraint_count = num_constraints;

    let h = profiler.span(Primitive::NttFft, || {
        QAP::witness_map_from_matrices::<E::ScalarField, D<E::ScalarField>>(
            &matrices,
            num_inputs,
            num_constraints,
            &full_assignment,
        )
    })?;

    let input_assignment = &full_assignment[1..num_inputs];
    let aux_assignment = &full_assignment[num_inputs..];

    // The contiguous buffer the MSM wants is built here, not in the MSM span:
    // it is part of preparing the scalars, and leaving it outside any span
    // would put a real cost — one 256-bit copy per witness element — into
    // `unattributed_ms` where nobody could say what it was.
    let (h_scalars, aux_scalars, assignment) = profiler.span(Primitive::FieldArith, || {
        let h_scalars = h.iter().map(|x| x.into_bigint()).collect::<Vec<_>>();
        let aux_scalars = aux_assignment
            .iter()
            .map(|x| x.into_bigint())
            .collect::<Vec<_>>();
        let input_scalars = input_assignment
            .iter()
            .map(|x| x.into_bigint())
            .collect::<Vec<_>>();
        let assignment = [&input_scalars[..], &aux_scalars[..]].concat();
        (h_scalars, aux_scalars, assignment)
    });

    let (h_acc, l_aux_acc, a_acc, b_g1_acc, b_g2_acc) = profiler.span(Primitive::Msm, || {
        (
            E::G1::msm_bigint(&pk.h_query, &h_scalars),
            E::G1::msm_bigint(&pk.l_query, &aux_scalars),
            E::G1::msm_bigint(&pk.a_query[1..], &assignment),
            E::G1::msm_bigint(&pk.b_g1_query[1..], &assignment),
            E::G2::msm_bigint(&pk.b_g2_query[1..], &assignment),
        )
    });

    let proof = profiler.span(Primitive::Other, || {
        // A = alpha + sum(a_i * z_i) + r*delta
        let mut g_a = pk.delta_g1.mul(r);
        g_a.add_assign(&pk.a_query[0]);
        g_a += &a_acc;
        g_a.add_assign(&pk.vk.alpha_g1);

        // B in G1, only needed when the proof is zero-knowledge.
        let g1_b = if r.is_zero() {
            E::G1::zero()
        } else {
            let mut g1_b = pk.delta_g1.mul(s);
            g1_b.add_assign(&pk.b_g1_query[0]);
            g1_b += &b_g1_acc;
            g1_b.add_assign(&pk.beta_g1);
            g1_b
        };

        // B in G2
        let mut g2_b = pk.vk.delta_g2.mul(s);
        g2_b.add_assign(&pk.b_g2_query[0]);
        g2_b += &b_g2_acc;
        g2_b.add_assign(&pk.vk.beta_g2);

        // C = s*A + r*B - r*s*delta + sum(l_i * z_i) + sum(h_i * t(x))
        let mut g_c = g_a * s;
        g_c += g1_b * r;
        g_c -= &pk.delta_g1.mul(r * s);
        g_c += &l_aux_acc;
        g_c += &h_acc;

        Proof {
            a: g_a.into_affine(),
            b: g2_b.into_affine(),
            c: g_c.into_affine(),
        }
    });

    Ok(Proved {
        proof,
        constraint_count,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::circuit::{membership_witness, poseidon_config, MerklePoseidonCircuit};
    use ark_bn254::{Bn254, Fr};
    use ark_crypto_primitives::snark::SNARK;
    use ark_groth16::r1cs_to_qap::LibsnarkReduction;
    use ark_groth16::Groth16;
    use ark_std::rand::rngs::StdRng;
    use ark_std::rand::SeedableRng;
    use ark_std::UniformRand;
    use zkprof_core::clock::{platform_clock, Clock};
    use zkprof_core::model::{System, Target};

    type Circuit = MerklePoseidonCircuit<Fr>;

    fn fixture(depth: usize) -> (Circuit, Fr) {
        let params = poseidon_config::<Fr>();
        let mut rng = StdRng::seed_from_u64(0x5EED);
        let leaf = Fr::from(3u8);
        let (root, siblings, positions) = membership_witness(&params, leaf, depth, &mut rng);
        (
            MerklePoseidonCircuit::blank(params, depth)
                .with_witness(root, leaf, siblings, positions),
            root,
        )
    }

    fn profiler(clock: &dyn Clock) -> Profiler<'_> {
        Profiler::new(
            clock,
            Target::native(),
            System::new("groth16", "bn254"),
            "test",
        )
    }

    /// The whole reason `prove_with_spans` may exist: it must produce the
    /// proof `Groth16::prove` would have. Same key, same randomness.
    #[test]
    fn matches_the_upstream_proof() {
        let (circuit, root) = fixture(4);
        let mut rng = StdRng::seed_from_u64(0x00C0_FFEE);
        let (pk, vk) = Groth16::<Bn254>::circuit_specific_setup(
            Circuit::blank(poseidon_config(), 4),
            &mut rng,
        )
        .unwrap();
        let (r, s) = (Fr::rand(&mut rng), Fr::rand(&mut rng));

        let clock = platform_clock().unwrap();
        let profiler = profiler(clock.as_ref());
        let ours = prove_with_spans::<Bn254, LibsnarkReduction, _>(&circuit, &pk, r, s, &profiler)
            .unwrap();

        let theirs = Groth16::<Bn254>::create_proof_with_reduction(circuit, &pk, r, s).unwrap();

        assert_eq!(
            ours.proof, theirs,
            "the span-split prover diverged from upstream"
        );

        let pvk = Groth16::<Bn254>::process_vk(&vk).unwrap();
        assert!(Groth16::<Bn254>::verify_proof(&pvk, &ours.proof, &[root]).unwrap());
    }

    #[test]
    fn every_primitive_is_attributed() {
        let (circuit, _) = fixture(4);
        let mut rng = StdRng::seed_from_u64(7);
        let (pk, _) = Groth16::<Bn254>::circuit_specific_setup(
            Circuit::blank(poseidon_config(), 4),
            &mut rng,
        )
        .unwrap();

        let clock = platform_clock().unwrap();
        let profiler = profiler(clock.as_ref());
        prove_with_spans::<Bn254, LibsnarkReduction, _>(
            &circuit,
            &pk,
            Fr::from(5u8),
            Fr::from(9u8),
            &profiler,
        )
        .unwrap();

        for primitive in [
            Primitive::WitnessGen,
            Primitive::NttFft,
            Primitive::FieldArith,
            Primitive::Msm,
            Primitive::Other,
        ] {
            assert!(
                profiler.primitive_us(primitive) > 0.0,
                "{primitive} recorded no time"
            );
        }
        assert!(
            profiler.primitive_us(Primitive::Hash) <= 0.0,
            "no Poseidon hashing is timed separately in this backend"
        );
    }

    #[test]
    fn the_proof_verifies() {
        let (circuit, root) = fixture(6);
        let mut rng = StdRng::seed_from_u64(1);
        let (pk, vk) = Groth16::<Bn254>::circuit_specific_setup(
            Circuit::blank(poseidon_config(), 6),
            &mut rng,
        )
        .unwrap();

        let clock = platform_clock().unwrap();
        let profiler = profiler(clock.as_ref());
        let proved = prove_with_spans::<Bn254, LibsnarkReduction, _>(
            &circuit,
            &pk,
            Fr::rand(&mut rng),
            Fr::rand(&mut rng),
            &profiler,
        )
        .unwrap();

        let pvk = Groth16::<Bn254>::process_vk(&vk).unwrap();
        assert!(Groth16::<Bn254>::verify_proof(&pvk, &proved.proof, &[root]).unwrap());
    }
}
