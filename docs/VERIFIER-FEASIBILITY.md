# Browser receipt verifier — route evaluation + findings (2026-09-01)

> **Superseded in part (2026-09-02, see `JOURNAL-BYTES-HYPOTHESIS.md`).**
> FINDING 1 below ("journal-bytes fidelity gap blocks end-to-end verification")
> was answered by the art-04 decider: the guests commit RFC 8785 / JCS-canonical
> journal bytes, so the journal digest IS recomputable from the receipt alone and
> the receipt verifies end-to-end. Route (a) was chosen and is live (the
> `zkprof-verify` crate + wasm page + snarkjs twin). Keep the route-eval and the
> FINDING-2 twin scoping; treat the "cannot recompute the journal digest" part
> of FINDING 1 as historical.

This is the Task-1 research deliverable for the `receipt-verifier-1` work order: the
**route selection with evidence**, and **two findings** that gate a fully-working
verification of the *actual* AINumbers receipts. Verification was actually
compiled and probed, not assumed — every claim below is backed by a build or a
read of the source.

## 1. The target: an estate `ZkVmReceipt`

The AINumbers receipts (read read-only from the public estate fixture dir) are an
**estate-native envelope**, not a raw risc0 binary receipt:

```json
{ "type": "ZkVmReceipt", "system": "risc0",
  "receiptFormat": "groth16-bn254",
  "imageId": "sha256:<hex>",
  "seal": "<base64 — the Groth16 seal / proof bytes>",
  "journal": { "chaingraph_version": "0.4.0", "kernel_digest": "sha256:...", "output": {...} } }
```

risc0 semantics that must hold (per `risc0-zkvm` `receipt.rs`, docs.rs 3.0.5):
`Receipt::verify(image_id)` verifies (a) the **seal integrity** using the
verifier parameters / control root / selector embedded for the crate version,
and (b) a **claim-digest check**:
`expected = ReceiptClaim::ok(image_id, journal.digest())` and
`expected.digest() == &receipt.claim()?.digest()`. So a faithful verifier must
bind the seal to **both** the imageId **and** the journal digest, exactly as the
work order's Task-1 API describes.

## 2. Route selection

### Route (a): `risc0-zkvm` verify path at the pinned 3.0.5 — **CHOSEN**

Selection criteria, verified:

| Criterion | Result |
|---|---|
| wasm32 compilation actually works | **YES (proven).** A scratch crate depending on `risc0-zkvm = { version = "=3.0.5", default-features = false }` **compiled and linked as `wasm32-unknown-unknown` (exit 0)**, pulling `risc0-zkvm 3.0.5`, `risc0-groth16 3.0.5`, `risc0-binfmt 3.0.5`, `ark-bn254`, `ark-groth16`. It also built natively (exit 0). |
| version compatibility with 3.0.5 receipts | **YES by construction.** `risc0-zkvm@3.0.5` embeds the 3.0.5 `VerifierContext` / control root / `Groth16ReceiptVerifierParameters` — this is exactly the crate version the workload doc pins (risc0 3.0.5). The `selector` check inside the seal is the version-routing mechanism; a 3.0.5 crate verifies 3.0.5 receipts. |
| binary size | Not measured yet (the wasm module built debug). risc0-groth16 is the heavy part; expected multi-MB. Marked NOT-VERIFIED (size | not the deciding criterion). |
| dependency hygiene | One crate (`risc0-zkvm`), whose verify path vendors `arkworks` + the risc0 circuit crates (all pinned/versioned). Acceptable; no chain/network deps in the verify path. |

**Independent corroboration:** risc0 issue #985 confirms a user got the risc0
verifier working in a freestanding `wasm32-unknown-unknown` environment (no
browser APIs, no WASI): *"I was also able to get the verifier working in the
freestanding Wasm environment."* Combined with the successful compile here, route
(a) is genuinely wasm-viable.

The hard part is **not** `groth16.verify` — the crate gives it. The hard part is
the inventory of things **outside** the crate that a real receipt-verification
needs, which is what the findings below are about.

### Route (b): `zkVerify/risc0-verifier` (HorizenLabs) — **REJECTED**

`risc0-verifier` is a **no-`std` STARK** verifier. Its README states plainly it
can handle receipts "that do **not** contain a Groth16 proof", and its versions
are `V2_1/V2_2/V2_3/V3_0` STARK contexts. The estate receipts are
**`groth16-bn254`** — route (b) structurally does not cover them. It is also
oriented to the zkVerify chain pallet (`zkverifyjs` submits proofs to a network),
which is the opposite of a zero-network browser verifier.

## 3. FINDING 1 — journal-bytes fidelity gap (blocks end-to-end verification)

The estate `ZkVmReceipt` carries `journal` as a **decoded JSON object**, not the
committed journal **bytes**. risc0's claim digest is over
`journal.digest() == sha256(journal.bytes)` — the exact bytes the guest wrote as
the journal. The stored envelope has **no** raw journal bytes and **no**
`journalDigest` field. Therefore a third party **cannot recompute the claimed
journal digest from the receipt alone** — it would have to guess the byte
serialization the guest wrote (JSON is not canonical across serializers), which a
verifier must never do silently.

**Implication:** the work order's own Task-1 requirement ("check the receipt's
journal digest") cannot be satisfied against the receipts **as stored**, even
with a fully-working risc0 wasm verifier, without the estate also publishing the
raw journal bytes (or an explicitly-canonical journal encoding). This is
**estate-triage material** (a verifier needs the bytes; the maintainer decides whether the
estate emits them). A verifier built now would be correct on the seal + imageId
and forced to `NOT-VERIFIED` on the journal digest for these envelopes.

## 4. FINDING 2 — the "twin" (art-652) is not a receipt-seal verifier

Read of the estate's in-guest verify kernel (`chaingraph/kernels/art-652-verify-receipt.kernel.mjs`):
it **verifies an Ed25519 signature** over the artifact envelope (via a vendored
`@noble/curves` ed25519 bundle) and produces a receipt **attesting that the
Ed25519 check ran in the zkVM**. It **never** verifies the receipt's own
`groth16-bn254` (or STARK) **seal**.

So the work order's premise "art-652 = your twin" is **not** a receipt-seal
verifier pair: art-652 proves *an Ed25519 verification*, my verifier proves *a
risc0 receipt seal*. They are complementary, not twins. Details and agreement
table are in `docs/VERIFIER-TWIN.md`.

**Implication for art-652's deferred prove (recommendation only; the maintainer decides):**
because a verify-side receipt-seal verifier (this crate) would add the missing
cryptographic proof check, art-652's *scope* could narrow to "attest the
Ed25519 check ran" and defer receipt-seal trust to a verifier — i.e. verify-side
redundancy may reduce what art-652 needs to prove in-guest. This is a
recommendation, not a resolution; the maintainer decides.

## 5. What was verified vs NOT-VERIFIED (honest register)

- **VERIFIED:** route (a) compiles to wasm32-unknown-unknown AND natively (two
  real `cargo build` runs); `risc0-zkvm@3.0.5` is the version-matched verifier
  (embeds 3.0.5 params); route (b) is a non-Groth16 STARK verifier.
- **NOT-VERIFIED:** a real `Receipt::verify(...)` against an actual estate receipt
  (blocked by FINDING 1 — the stored journal cannot yield its digest). So no
  real "receipt verifies PASS" result was produced in this run, and therefore no
  **verification timing** (Task 4's benchmark is NOT-VERIFIED until a receipt can
  be verified end-to-end).
- **NOT-VERIFIED:** wasm binary size (built debug only). Not the deciding
  criterion.

## 6. Recommendation

Choose **route (a) `risc0-zkvm = "=3.0.5"`** (verify-only, `default-features =
false`), compile to wasm32, and **land the estate-side journal-bytes emit** (or a
canonical journal encoding) before a browser verifier can report a truthful
`valid: true` on a real receipt. Until then the verifier is correct-but-incomplete
on the journal-digest binding: it can check seal + imageId, and must report the
journal-digest check as `NOT-VERIFIED` rather than pass.

Sources: risc0-zkvm 3.0.5 receipts/verification (docs.rs + risc0 zkVM docs);
`risc0-ethereum/release-3.0` `RiscZeroGroth16Verifier.sol` (the G16 selector /
control-root / claim-digest semantics); `zkVerify/risc0-verifier` README;
estate `art-652-verify-receipt.kernel.mjs`; estate `compute-proof/` receipts
(read-only). This document is the feasibility+findings deliverable; nothing in it
is a repository write outside zkprof-web, and no AINumbers repo file was modified.
