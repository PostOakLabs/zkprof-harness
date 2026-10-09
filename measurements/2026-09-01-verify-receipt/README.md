# Verify receipt — first real cryptographic workload row (2026-09-01)

**Single machine, single session, single sample. NOT a benchmark.** One estate
`groth16-bn254` receipt (art-01, imageId a1a0bc89…, 256-byte Groth16 seal over a
JCS-canonical journal digest). Same verification code path in two targets.

## Environment

| | |
|---|---|
| CPU | Intel Core i9-10850K @ 3.60 GHz (10c/20t) |
| Native | target/release `zkprof-verify --bin hypothesis`, GNU x86_64, release |
| wasm | headless Chrome 151.0.7922.175, `--headless=new`, risc0-zkvm 3.0.5 → wasm32-unknown-unknown (release), wasm-bindgen 0.2.127 |
| Order | native release measured from the crate bin's JCS-verify timing; wasm measured via `window.verify` (the page's exposed binding) timed with `performance.now()` |

## Result

| Target | Verify (JCS, valid) | note |
|---|---|---|
| native, release | **~3.8–4.1 ms** | per receipt; the crate's `run_risc0_verify` wall time |
| native, debug (PR-20) | ~75–102 ms | debug is unoptimized; the debug/release ratio implied is ~18–27× (endpoints: 75/4.1=18.3× to 102/3.8=26.8×), not a narrower ~20–25× |
| wasm in-browser, cold | 35.2 ms | first call includes wasm module compile + JIT warmup |
| wasm in-browser, warm | **~14.3–14.7 ms** (5 reps; steady) | headless Chrome, single machine |
| wasm in-browser, tampered seal | ~5.7 ms | the invalid-proof path fails in the pairing check; NOT a comparable valid-verify number |

**Headline:** wasm in-browser verify is ~3.5–3.9× slower than the native
release build on this machine (~14.5 ms vs ~4.0 ms, a range the table's
endpoints alone give: 14.3/4.1 = 3.49× to 14.7/3.8 = 3.87× — the
"3.5–3.75×" figure in an earlier draft was narrower than its own endpoints),
and ~14 ms is comfortably interactive. The cold-start penalty (~35 ms) is the
wasm module compile + first call.

## Honesty register

- Single sample each; no repeats across sessions; same machine. The "5 reps;
  steady" warm figure is a dispersion claim with no sd/CI recorded — "steady"
  is an eyeball, not a measured variance.
- Native release was measured via the crate's `run_risc0_verify` timing, not the
  browser; wasm was measured in-browser via `performance.now()` around the
  exposed `verify()` binding. They measure the same cryptographic work but on
  different runners (native binary vs wasm-in-headless-Chrome).
- This is the repository's first real cryptographic workload row (previously all
  numbers were proving/attribution rows of the arkworks Groth16 path, not a
  risc0 receipt verification). Not a benchmark, so nothing is compared to a
  third-party system.
- The crate's per-check `ms` is 0.0 on **both** targets — the wasm note's
  "no monotonic clock on wasm32" rationale is not the cause, and is 0.0 natively
  too (no `Instant` per-check in the crate's `verify` path). The page itself
  does not measure per-check wall ms either; the timing above (and the earlier
  ~75–102 ms debug figure) was taken by an external harness timing the exposed
  `window.verify` binding, not by the page. No committed artifact backs a
  per-check `ms`.
