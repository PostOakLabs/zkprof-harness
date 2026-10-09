# zkprof-verify

A client-side verifier for risc0 `groth16-bn254` receipts (route A: `risc0-zkvm`
3.0.5 verify-only, compiled to wasm32). It verifies the receipt seal against a
claim derived from an image ID and a journal digest, and names every binding
check individually so a failure says *which* binding broke.

**Honesty status:** this crate is the empirical journal-bytes hypothesis harness
(`docs/JOURNAL-BYTES-HYPOTHESIS.md`). It is a research/verification instrument,
not a production verifier promise: it verifies whatever the receipt's current
byte contract actually turns out to be. See the crate's `verify` doc and that
doc's per-receipt table.

Zero egress preserved: the ceremony verification key and control root are
embedded in-crate (via risc0-zkvm 3.0.5's `VerifierContext`), the wasm module is
inlined/self-hosted by the page, and nothing is fetched at runtime.

Note: the eqtylab/risc-zero-verifier repository (Apache-2.0, dead at risc0
0.20.1) was consulted as a **packaging / wasm-pack / page-wiring skeleton only**,
never for its verifier core and never for its npm packages. See CREDITS.

Status: verification crate under construction / hypothesis harness live.

## Remediation notice (2026-09-02)

Crate-side accepted items are landed (see the commit): the redundant
"claim digest == journal digest" check row was removed (it re-printed the seal
boolean), the dead `JournalCandidate.verified` field and the duplicated
candidate list were removed (one list, `journal_candidate_bytes`), the two
RFC-8785 divergences are documented in `canonical_jcs`, and a
`type` / `system` / `receiptFormat` gate now fail-closes on unsupported values.

**Deferred (with reason), not skipped:**

- **Page-side verifier-identity items (VER-1/2/6/9/15/16/17/18):** deferred with
  the wasm-repro group. The page (`web/verify/index.html` + its regenerated
  `pkg/*`) cannot be rebuilt in this environment: the wasm-bindgen glue and wasm blob
  are gitignored/absent, so any page edit whose rebuilt artifact is not
  produced would be a staleness class this remediation exists to kill. The
  input field + known-good imageId allowlist, the `.slice(8)` → `.slice(9)`
  tamper fix, the `ms` column removal, the serde `serialize_maps_as_objects`
  fix (crate-side method shipped here; the page still uses the old glue until
  rebuilt), and the VER-15..18 UI fixes move to a wasm-capable follow-on
  work order.
- **Re-measurement (MEAS-1):** deferred. Needs the browser harness; the
  committed mis-scaled `marshal-*.json` files must be superseded with real
  re-measured values, never patched in place. Runs after R1/R2.
