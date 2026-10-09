# The combined receipt-bundle verifier (RV5)

One command verifies both halves of a portable receipt bundle offline: the
zk proof half and the transparency-log half. The bundle FORMAT is owned by
`PostOakLabs/ainumbers` (`chaingraph/standard/RECEIPT-BUNDLE.md`, v0.1); this
repository owns the reference verifier.

```sh
node web/bundle/verify-bundle.mjs crates/zkprof-verify/fixtures/bundles/real-art-04.bundle.json
```

```
== real-art-04.bundle.json
bundle_version: 0.1
PROOF HALF : PROOF-VALID
LOGGED HALF: LOGGED
  log-half subject: registry-checkpoint ainumbers.co/registry/lineage (policy chaingraph/policies/ainumbers-registry-lineage.tlog-policy)
  v0.1 scope: the log half commits to the anchored subject above, NOT to the receipt itself.
COMBINED   : proof=VALID, logged=VALID — two independent claims, stated separately; neither implies the other
```

Exit 0 requires both halves to pass; the two verdicts remain separately legible
in both human and `--json` output.

## The two halves, and why they stay separate

**PROOF half** is the snarkjs independent twin (`web/twin/verify.mjs`,
unchanged): the risc0 3.0.5 Groth16/BN254 pairing check over
`ReceiptClaim::ok(imageId, journalBytes)` with `journalBytes = utf8(JCS(journal))`
per SPEC.md §18.7. Nothing about it reads the anchor.

**LOGGED half** is reimplemented in this repository (`web/bundle/verify-bundle.mjs`)
from the C2SP/Sigsum primary wire formats — RFC 6962 Merkle inclusion walk,
signed-note checkpoint framing, the `cosignature/v1` message, the Sigsum leaf
serialization and checkpoint-origin derivation. It shares no runtime code with
the estate's own `register-sigsum.mjs`, which is the point: like the twin, it
is an independent implementation that agrees with the estate's, not a call
into it. It verifies, in order: the bundle's trust policy is one this verifier
pins (name + content hash); `sha256(checkpoint_text) == anchored_hash`; the
checkpoint's own signature against the **pinned subject-log operator key**;
the Sigsum leaf commits to `anchored_hash`; the RFC 6962 inclusion walk
reconstructs the tree head root; the log's checkpoint signature verifies
against the **pinned** log key (origin derived from the key, never read from
the record); and the `sigsum-generic-2025-1` 2-of-3 witness quorum is met by
**valid, pinned, hash-matched** cosignatures (the published records carry 12
distinct cosigners; only pinned matches count). Nothing about it reads the
seal.

**The non-collapse rule.** "Proof valid" and "logged" are different statements
about different subjects; neither implies the other. The verifier's output
states both explicitly and never emits a single claim covering both — the
machine verdict carries `proof` and `logged` as siblings, and the test asserts
structurally that no collapsed field exists. The RED fixtures exist to prove
the failure modes are real and half-local: a mutated seal fails the proof half
while the logged half still passes; a mutated inclusion-proof path hash fails
the logged half while the proof half still passes.

## Honest scope (v0.1)

The published anchor records the estate has shipped commit to **registry
checkpoints** (`ainumbers.co/registry/lineage` and `/errata` log heads), not to
individual receipts. The verifier states this on every run. When a
per-artifact (`subject.kind: "execution_hash"`) Sigsum record is published by
the estate, the same anchor member carries it and the same verifier gains one
binding step — that is a registration event on the estate side, not a format
or verifier redesign. See bundle SPEC §3.4.

## Verification status of the verifier itself

- `node web/bundle/bundle.test.mjs` — 25 assertions, all passing at land time;
  the RED controls fail in exactly their own halves (output quoted in the
  landing PR body).
- The five fixtures are copy+derive constructions from published bytes only
  (`crates/zkprof-verify/fixtures/bundles/README.md` names every source and
  the commit they were read from). No proving, no signing, no registration
  happened in this repository to produce them.
- Not verified here, honestly: the CLI's behavior under adversarial bundle
  shapes beyond the two RED controls (deeply malformed JSON, missing members)
  is covered only by the guards shown; a fuzz pass is a possible follow-up.
  Node's WebCrypto is the only crypto provider; the noble bundle is used by
  the twin for BN254 only.
