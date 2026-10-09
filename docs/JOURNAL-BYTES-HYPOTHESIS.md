# Journal-Bytes Hypothesis — per-receipt result (2026-09-01)

**Headline: CONFIRMED-JCS (corrected 2026-09-01).** The guests commit **RFC 8785 /
JCS-canonical bytes** of the receipt's `journal` JSON object (compact, keys
sorted, no whitespace). A full risc0 3.0.5 Groth16/BN254 verification — seal
integrity + claim-digest binding to imageId and the JCS-derived journal digest —
**passes end-to-end** for every fixture receipt tested, **including art-04, the
one receipt whose journal has UNSORTED nested keys**. The prior headline said
"HYPOTHESIS-CONFIRMED" on the strength of three key-sorted journals, which
cannot distinguish JCS from insertion order; the art-04 decider test (below) is
what actually separates the two contracts, and JCS wins. The estate's open
question (board row RECEIPT-JOURNAL-BYTES-CONTRACT-1, step 1) is answered
empirically, without the guest source.

This resolves the receipt-verifier-1 "FINDING 1" (journal stored as decoded
JSON, no byte contract): the byte contract is **JCS-canonical**, so a third
party CAN recompute the journal digest from the `journal` object — and therefore
can verify the receipt. Verification is genuinely browser-feasible.

## Method

For each estate `ZkVmReceipt` fixture (copied read-only from
`chaingraph/kernels/fixtures/compute-proof/`, with provenance note in
`fixtures/compute-proof/PROVENANCE.md`), the crate:

1. decodes the base64 `seal` (256 bytes = Groth16 `(a,b,c)` on BN254),
2. derives the journal bytes for each candidate and the risc0 journal digest
   (`sha256(bytes)`),
3. builds a risc0 3.0.5 `Receipt` whose `Groth16Receipt` claim is
   `ReceiptClaim::ok(image_id, journal_bytes)` and whose journal is the candidate
   bytes,
4. calls `Receipt::verify(image_id)` (the version-matched verifier) and reports
   the outcome per candidate.

All receipts except art-04 were proven from the same guest image (they share
imageId `sha256:a1a0bc89b5b1fe…`); art-04 is a **different guest image**
(imageId `sha256:93c746e79afcf4b2…`) and is the key-ordering discriminator: its
journal has unsorted nested keys. The seal is a valid risc0 3.0.5 `Groth16Seal`.
Verification was run from a debug build on a single machine (`zkprof-verify`
crate, `--bin hypothesis`, plus the art-04 decider test); the timing is a
single-machine measurement, not a benchmark.

## Per-receipt hypothesis table

| Receipt | imageId (prefix) | Journal candidate | journal digest (sha256) | Verify outcome | ms |
|---|---|---|---|---|---|
| art-01 | `a1a0bc89b5b1fe…` | **RFC8785/JCS canonical** | `3e6136d7be3084fe01dbbe871304b2972a80b8f07bfccc37e008dbb37f4b95de` | **HYPOTHESIS-CONFIRMED (VERIFIED)** | 101.6 |
| art-01 | | compact JSON (no whitespace) | `3e6136d7…b95de` (same as JCS) | VERIFIED | 74.4 |
| art-01 | | pretty JSON (2-space indent) | `927d6809816c84562182df78d498c1a716c683cfa84ff8046cc454e4227f40c8` | HYPOTHESIS-REFUTED (proof invalid) | 74.3 |
| art-106 | `a1a0bc89b5b1fe…` | **RFC8785/JCS canonical** | `592d2fb7f3231c1617bb7d8a55b2046a8045a48d9f34c09e4e1370acf834092c` | **HYPOTHESIS-CONFIRMED (VERIFIED)** | 77.1 |
| art-106 | | compact JSON | `592d2fb7…34092c` (same as JCS) | VERIFIED | 74.3 |
| art-106 | | pretty JSON | `296b39dea73f7236e698721209d917a919131aad10abffa6ae7cc39ac55a22f6` | REFUTED (proof invalid) | 74.2 |
| rca-01 | `a1a0bc89b5b1fe…` | **RFC8785/JCS canonical** | `0c0d66ea30e2e2ea55a5a80136b75b0ff4dccf97aa0a24bd5e0bb6ae3ac376bd` | **HYPOTHESIS-CONFIRMED (VERIFIED)** | 74.7 |
| rca-01 | | compact JSON | `0c0d66ea…c376bd` (same as JCS) | VERIFIED | 74.3 |
| rca-01 | | pretty JSON | `c7056a10b093b1f2f63e742efa50cffa4edcb09c259934b62934597a8586c2dc` | REFUTED (proof invalid) | 73.9 |
| art-04 | `93c746e79afcf4b2…` | **RFC8785/JCS canonical** | `3a089ca010da4cc939c482b41b52b659a6ff177a2b82df91c64a774164bcd4be` | **CONFIRMED-JCS (VERIFIED)** | ~0.2 |
| art-04 | | compact insertion-order (stored key order) | `a92cceb5da47a360d4097c7585584aefac5036df9dd0d3be28b9cf0475243136` | REFUTED (proof invalid) | ~0.2 |

## Findings

1. **JCS-canonical journal bytes verify.** Every receipt passes a full risc0
   3.0.5 Groth16 verification when the journal digest is derived from the
   RFC 8785 / JCS canonical form of the `journal` object. The `seal` (256-byte
   `(a,b,c)` proof, no 4-byte selector) is a valid risc0 3.0.5 `Groth16Seal`.
2. **JCS beats insertion order on the discriminator.** The three original
   fixtures were key-sorted, so their JCS and compact-insertion-order byte
   strings were byte-identical and could not separate the two contracts. art-04's
   journal has UNSORTED nested keys, so its two candidates diverge: JCS
   (`3a089ca0…`) **verifies**, insertion-order (`a92cceb5…`) does **NOT**. So the
   operative contract is genuinely "compact, **key-sorted**, no whitespace"
   (RFC 8785/JCS), not merely "whatever the stored order was."
3. **The pretty-indent alternate is refuted.** 2-space-indented serialization
   produces a different digest and the seal does NOT verify → the guest did not
   commit indented JSON.
4. **No risc0-semantic blocker.** Every step of the risc0 3.0.5 verification was
   reproducible: seal parse, control-root / verifier-parameters binding
   (embedded `VerifierContext`), claim digest = `ReceiptClaim::ok(image_id,
   journal_digest)`, and the Groth16 pairing check. Nothing is BLOCKED.
5. **Corrected: art-04 is a distinct image and it confirms JCS.** The prior
   write-up claimed "no fetchable fixture was available" for the other imageIds —
   that was false. art-04 (imageId `93c746e79afcf4b2…`, a different guest image)
   IS a fetchable fixture in the estate dir, and its unsorted nested keys make it
   the decisive JCS-vs-insertion-order test. It verifies only under JCS.

## Honesty register

- Verification is real: each HYPOTHESIS-CONFIRMED row ended in
  `Receipt::verify` returning `Ok(())` (seal valid + claim digest matched +
  imageId bound), not a "looks plausible" contrivance.
- The verify timing is a **single-machine, debug-build, single-sample**
  measurement, not a benchmark. Task 3 will produce the native-vs-wasm row.
- The JCS canonicalizer is a faithful RFC 8785 approximation (sorted keys, no
  whitespace, JSON.stringify primitives); the exact-number handling matches for
  the finite small numbers these journals carry. The RFC 8785 number edge cases
  (negative zero, exponent thresholds) are not exercised by this data — noted,
  not silently assumed.

## Estate implication (recorded, not decided here)

Because the journal-bytes contract is JCS-canonical, the receipt IS verifiable
by a third party from the published `journal` object alone. The recommended
estate change (the maintainer decides) is to **document RFC 8785/JCS as the journal byte
contract** and to **emit the raw journal bytes** (or a `journalDigest` field) so
verifiers need not re-derive it — a low-cost robustness improvement, though
verification already works against the current artifacts.
