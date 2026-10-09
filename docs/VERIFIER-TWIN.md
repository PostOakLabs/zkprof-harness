# Verifier twin cross-check — receipt-seal verifier vs estate art-652 (2026-09-01)

> **Superseded in part (2026-09-02, see `JOURNAL-BYTES-HYPOTHESIS.md`).** The
> "journal digest: NOT-VERIFIED (FINDING 1)" row below is no longer accurate.
> Finding 1 of `VERIFIER-FEASIBILITY.md` (the journal is stored as decoded
> JSON, so its digest cannot be recomputed) was answered empirically by the
> art-04 decider: the guests commit **RFC 8785 / JCS-canonical bytes**, so a
> third party CAN recompute the journal digest from the `journal` object, and
> the receipt verifies end-to-end. Read this document for the twin-vs-art-652
> scoping (VERIFIER-TWIN-AGREEMENT.md has the live cross-verifier table); treat
> the `journal`-digest NOT-VERIFIED row as historical.

**Task 3 result.** The work order asked me to cross-check my receipt verifier
against "the estate's art-652 kernel logic." **Reading the kernel produced a
FINDING, not an agreement table: art-652 is not a receipt-seal verifier.** The
two tools verify *different artifacts*, so there is no cryptographic agreement
(or disagreement) to record between them — and that gap is itself the finding.

## FINDING: art-652 verifies an evidence envelope, not a risc0 receipt seal

Read of `chaingraph/kernels/art-652-verify-receipt.kernel.mjs` (compute() →
verifyReceipt()):

- The "receipt" art-652 validates has **`schema: 'ainumbers.evidence.v0.1'`** — an
  **Evidence Envelope v0.1**, NOT the risc0 `ZkVmReceipt` my Task-1 verifier
  targets (`type: ZkVmReceipt / system: risc0 / receiptFormat: groth16-bn254`).
- Its checks are a signature + shape + chain-link set, all over the *envelope*:
  `RECEIPT_MISSING`, `SCHEMA_INVALID` (== `ainumbers.evidence.v0.1`),
  `HASH_FIELD_FORMAT_INVALID` (`input_hash`/`policy_digest`/`execution_hash`/
  `output_hash` — SHA-256 tagged), `NO_SIGNATURES`, `SIGNING_PREIMAGE_ERROR`,
  `ALG_UNSUPPORTED` (must be `EdDSA`), `KID_NOT_RESOLVABLE` (did:key),
  `SIGNATURE_VALID` / `SIGNATURE_INVALID` / `SIGNATURE_MALFORMED` (ed25519,
  `zip215:false`), `PREV_LINK_OK`/`PREV_LINK_MISMATCH`/`PREV_LINK_UNVERIFIED`
  (previous-receipt chain hash).
- It **never** verifies the receipt's cryptographic **seal**. And its own compute
  proof (the `ZkVmReceipt` attesting that the Ed25519 check ran in-guest) is
  emitted as **`compute_proof_ready: 'deferred'`** — i.e. not yet produced.

So the estate's "verify receipt" tool checks *"is this evidence envelope's
Ed25519 signature valid and its chain link intact"*; my verifier checks *"is this
risc0 `groth16-bn254` seal a valid proof binding imageId + journal digest"*.
Different artifacts, different claims, different crypto (Ed25519 vs Groth16/BN254
STARK-to-SNARK).

## Why there is no agreement table

A cross-check needs the two sides to verify the *same* input. art-652 consumes an
`ainumbers.evidence.v0.1` envelope; my verifier consumes a `ZkVmReceipt`. The
estate's `compute-proof/` fixtures are `ZkVmReceipt`s — which art-652 would reject
at `SCHEMA_INVALID` (its schema is `ainumbers.evidence.v0.1`, not `ZkVmReceipt`).
Conversely, an evidence envelope has no risc0 seal for my verifier. No fixture is
valid for both, so no per-case CONFIRMED / PARTIAL / NOT-IN-PAPER verdict can be
computed between them. That is the honest result, stated rather than forced into
a table.

### Scoping / "agreement" table (what each verifies)

| Case | estate art-652 (`ZkVmReceipt` input?) | my receipt-seal verifier (`ZkVmReceipt` input?) |
|---|---|---|
| risc0 `ZkVmReceipt` fixture | **not valid** — `SCHEMA_INVALID` (expects `ainumbers.evidence.v0.1`); ignores the seal | **is the target** — verifies seal + imageId + (attempts) journal digest |
| `ainumbers.evidence.v0.1` envelope | **is the target** — verifies Ed25519 signature over JCS preimage, hash-field shape, prev-link | **not applicable** — no risc0 seal to verify |
| tamper the Ed25519 signature | `SIGNATURE_INVALID` / `SIGNATURE_MALFORMED` | n/a |
| tamper the risc0 `seal` | n/a (never reads it) | (would be) `seal invalid` |
| `imageId` / `image_id` binding | n/a | imageId recognition/expected check |
| `journal` digest | n/a | NOT-VERIFIED (FINDING 1: stored as JSON, not bytes) |
| compute proof (`compute_proof_ready`) | emitted `deferred` | the thing this verifier provides |

## What this implies for art-652's deferred prove (recommendation only — the maintainer decides)

Because a verify-side receipt-seal verifier (this crate) adds the *missing*
cryptographic proof check over the `ZkVmReceipt`, art-652's scope could **narrow**:
art-652 need only attest "the Ed25519 evidence-envelope check ran", and **defer
receipt-seal trust to the verifier**. In other words, verify-side redundancy may
reduce what art-652 must prove in-guest (it currently marks its own compute proof
`deferred` anyway). This is a **recommendation**, not a resolution — the maintainer decides
whether the estate adopts a browser-side receipt-seal verifier and narrows
art-652 accordingly. One of the two designs may be redundant going forward: the
estate's deploy-side Ed25519-verify-asserted-by-a-receipt, versus a browser-side
true-receipt-seal verifier.

## Honest register

- No real cryptographic verification was executed here (see
  `docs/VERIFIER-FEASIBILITY.md` FINDING 1 — the estate `ZkVmReceipt` stores the
  journal as decoded JSON, not committed bytes, so the risc0 journal digest
  cannot be recomputed from the receipt alone). So neither my verifier nor a
  risc0-native run produced a `valid: true` on an actual estate receipt in this
  run.
- The twin read is from the estate source (read-only); no AINumbers repo file was
  modified, and nothing here is a write outside zkprof-web.
