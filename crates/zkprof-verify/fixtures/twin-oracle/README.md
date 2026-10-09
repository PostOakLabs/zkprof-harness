# twin-oracle — receipt-verifier-4 twin oracle dump

Produced by `cargo run --bin dump -- crates/zkprof-verify/fixtures/compute-proof/*`
(crate `zkprof-verify`, Task 1). Every field is the **crate/risc0** side of a
comparison: the JS twin (`web/twin/verify.mjs`) reads the fixed constants here,
derives the claim digest / public inputs **in JS**, and runs the pairing
independently. This directory is a static **data** artifact (no derivation
code), so reading it from JS keeps the twin independent of the crate at runtime.

## Files

| File | Contents |
|---|---|
| `art-01.oracle.json` | decoded primitives for `compute-proof/art-01.receipt.json` |
| `art-106.oracle.json` | ditto `art-106.receipt.json` |
| `rca-01.oracle.json`  | ditto `rca-01.receipt.json` |
| `crate-verdicts.json` | the crate's `zkprof_verify::verify` verdict over every case in `.../twin-cases/` |
| `README.md`           | this file |

`crate-verdicts.json` is written by `cargo run --bin crate_verdicts` and is the
`crate` column of `docs/VERIFIER-TWIN-AGREEMENT.md`.

## Field description (per `*.oracle.json`)

All coordinates are for the BN254 groups over the extension basis shared by
ark-bn254 and the noble bundle (verified: the generator and the `b` constant are
identical in both). G1 point `{x,y}` and G2 point `{x_c0,x_c1,y_c0,y_c1}`; every
coordinate is a decimal bigint string.

| Field | Type | Meaning |
|---|---|---|
| `source` | string | original fixture file name |
| `image_id` | string | `sha256:<hex>` image digest the claim binds (`pre`) |
| `image_id_hex` | string | the 64-hex digest without the `sha256:` prefix |
| `journal_digest` | string | risc0 sha256 (`digest_of`/`Journal::digest`) of the RFC 8785/JCS bytes of the `journal` object, as `sha256:<64hex>` |
| `journal_digest_hex` | string | the same, `sha256:<64hex>` (kept for parity) |
| `claim_digest` | string | `ReceiptClaim::ok(image_id, JCS bytes).digest()` as 64-hex |
| `control_root` | string | `VerifierContext::default().groth16_verifier_parameters.control_root` (64-hex, fixture-independent) |
| `bn254_control_id` | string | same parameters' `bn254_control_id` (64-hex, fixture-independent) |
| `public_inputs` | array(5) | the Groth16 public inputs in the **correct** order `[a0, a1, c0, c1, id]`, decimal strings. `(a0,a1)=split_digest(control_root)`, `(c0,c1)=split_digest(claim_digest)`, `id = fr(rev(bn254_control_id)) = LE(control_id) mod r`. |
| `proof.A` | G1 | seal `a` affine coordinates |
| `proof.B` | G2 | seal `b` affine coordinates |
| `proof.C` | G1 | seal `c` affine coordinates |
| `verifying_key.alpha` | G1 | `alpha_g1` |
| `verifying_key.beta` | G2 | `beta_g2` |
| `verifying_key.gamma` | G2 | `gamma_g2` |
| `verifying_key.delta` | G2 | `delta_g2` |
| `verifying_key.ic` | array(6) | `gamma_abc_g1` — `ic[0]` is the constant term, `ic[1..5]` pair with `public_inputs[0..4]` |

## Semantics notes

- `split_digest(d)`: `big_endian = reverse(d_bytes)`; returns
  `(Fr(big_endian[16..32]), Fr(big_endian[0..16]))` — i.e. `a0 = Fr(rev(lower 16
  bytes))`, `a1 = Fr(rev(upper 16 bytes))`. This order is the **correct** one; a
  stale earlier dump had the two halves swapped.
- The 256-byte seal is `A(x||y, BE 32 each) ++ B(x_c0,x_c1,y_c0,y_c1, BE 32
  each) ++ C(x||y, BE 32 each)`; the G2 order in the seal bytes is
  `[x_c1, x_c0, y_c1, y_c0]`, matching risc0 `g2_from_bytes`.
- The vkey is the default risc0 ceremony key; the oracle decodes it via a
  serde round-trip into the arkworks `VerifyingKey<Bn254>` and reads the public
  `alpha/beta/gamma/delta/ic` fields.
