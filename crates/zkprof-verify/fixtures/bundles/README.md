# Receipt-bundle fixtures (RV5)

Five `RECEIPT-BUNDLE` v0.1 fixtures for `web/bundle/verify-bundle.mjs`, the
combined proof+transparency verifier. The bundle format is specified by the
artifact owner, `PostOakLabs/ainumbers`, at
`chaingraph/standard/RECEIPT-BUNDLE.md`; these files follow it.

## Construction (copy + derive, never prove, never register)

Every byte in the three `real-*` bundles comes from a published artifact of
`PostOakLabs/ainumbers` at commit `b5172d4a` (origin/main, 2026-09-03), or is
derived from one:

| bundle member | source |
|---|---|
| `receipt.*` | `chaingraph/kernels/fixtures/compute-proof/<name>.receipt.json`, verbatim (`provenance.receipt_source` names the file; `receipt_source_sha256` pins its bytes) |
| `journal_contract.*` digests | derived: `sha256(utf8(JCS(journal)))` and the ReceiptClaim digest, per SPEC.md §18.7 — advisory annotations the verifier recomputes and never trusts |
| `anchor.record` | `registry/lineage/checkpoint.sigsum-record.json` or `registry/errata/checkpoint.sigsum-record.json`, verbatim (public Sigsum transparency-log data) |
| `anchor.checkpoint_text` | `registry/<log>/checkpoint`, verbatim (the signed-note text `anchored_hash` commits to) |
| `trust_policy.*` | transcribed from `chaingraph/policies/sigsum-generic-2025-1.tlog-policy` (`sha256` is of that file) |

The two `red-*` controls are byte-mutated copies of `real-art-04` (one base64
character of the seal; one hex digit of the first inclusion-proof path hash),
with a `_red_control` member naming the mutation. They exist to prove the
verifier fails honestly and that the two halves are independently decidable:
each RED control fails in exactly its own half while the other half still
passes.

## The fixtures

| file | receipt | anchor subject | expected |
|---|---|---|---|
| `real-art-04.bundle.json` | art-04-agent-identity-attestation-checker (imageId `sha256:93c746e7…`; the SPEC.md §18.7 discriminating journal — unsorted nested keys) | lineage checkpoint, size 1 | PROOF-VALID + LOGGED |
| `real-art-01.bundle.json` | art-01-ap2-mandate-chain-validator | lineage checkpoint, size 1 | PROOF-VALID + LOGGED |
| `real-art-106.bundle.json` | art-106-tempo-subscription-reconciler | errata genesis checkpoint, size 0 (exercises the signed-note extension-line path) | PROOF-VALID + LOGGED |
| `red-seal-art-04.bundle.json` | art-04, seal mutated | lineage | PROOF-INVALID (pairing check), LOGGED still passes |
| `red-inclusion-art-04.bundle.json` | art-04 | lineage, `inclusion_proof.path[0]` mutated | PROOF-VALID still passes, NOT-LOGGED (inclusion walk) |

Run the verifier and the test:

```sh
node web/bundle/verify-bundle.mjs crates/zkprof-verify/fixtures/bundles/*.bundle.json
node web/bundle/bundle.test.mjs
```

Honest scope note (bundle SPEC §3.4): the log half of every fixture commits to
the **registry checkpoint** named in `anchor.subject`, not to the receipt
itself — that is the anchoring the estate has published; per-receipt
`execution_hash` anchoring is the named follow-on. The combined verdict never
collapses "proof valid" and "logged" into one claim.
