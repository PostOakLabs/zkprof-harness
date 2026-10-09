# zkprof-backend-arkworks

Groth16 over BN254, instrumented: a real circuit, and `ark-groth16`'s prover
taken apart so that each stage's time is attributed to a primitive.

## What is here

| module | what it does |
|--------|--------------|
| `circuit` | depth-parameterised Merkle-Poseidon membership circuit; `243 * depth + 1` constraints |
| `prover` | `prove_with_spans` — the upstream prover, re-implemented with spans |
| `bench` | `benchmark` — builds the workload, sizes it, verifies the proof, emits a report |

## The circuit

One Poseidon permutation per tree level, over a rate-2/capacity-1 sponge with
the 128-bit parameter set for three BN254 field elements (8 full rounds, 57
partial, `x^5`). The default depth is 64, i.e. 15,553 constraints. Depth is a
parameter, not a constant, so the same code profiles a circuit of any size.

The tree is never materialised. Only the authentication path is needed to
build a witness, so a depth-`d` instance costs `d` hashes to construct rather
than `2^d`.

## Why the prover is re-implemented

`Groth16::create_proof_with_reduction` is one opaque call with internal timers
this crate cannot read. To attribute time, the call has to be taken apart — and
that risks changing its answer. Two things guard against that:

- `matches_the_upstream_proof` proves the same circuit with the same key and
  the same `r`/`s` through both paths and asserts the proofs are equal.
- `benchmark` verifies the proof before it will emit a report. A prover that
  produces a beautifully formatted measurement of the wrong thing produces no
  report at all.

## Primitive attribution, honestly

`witness_gen` covers synthesis, LC inlining, sparse matrix construction and
witness evaluation: four distinguishable things, in one span, because arkworks
exposes one call. It is also where the circuit's Poseidon permutations are
spent — they are **not** attributed to `hash`, because there is no hook inside
`generate_constraints` to time them separately and adding one means patching
upstream.

`other` covers the scalar multiplications by `r` and `s`, the group operations
that assemble C, and the affine conversions. It is measured, not a residual.

Time that is in the measured region but in no span — mostly the deallocation
of the constraint matrices at the end of a proof — stays in
`unattributed_ms`. It is printed and it is in the report.

## No results

This crate contains no benchmark numbers. It measures what you run it on.

## License

Apache-2.0. See [LICENSE](../../LICENSE).
