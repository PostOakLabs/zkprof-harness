# CREDITS

The verifier core is `risc0-zkvm` 3.0.5 (Apache-2.0) — the estate-pinned,
version-matched verifier; the ceremony verification key and control root are
embedded via its `VerifierContext` (no external fetch, no npm packages).

**eqtylab/risc-zero-verifier** (Apache-2.0; archived at risc0 0.20.1, dead)
was consulted as a **packaging / wasm-pack / browser-wiring reference only**, per
the maintainers' route decision of 2026-09-01. **No code was copied from it, no npm
package from it is used, and its verifier core is not used anywhere.** Its only
role was the pattern for how a risc0 verifier can be packaged for the browser;
this crate's verifier is entirely `risc0-zkvm` 3.0.5.

The wasm-pack / npm / page-wiring patterns referenced general wasm-bindgen
practice and risc0's own browser-verify examples; nothing from eqtylab's wasm
artifact is vendored or linked.
