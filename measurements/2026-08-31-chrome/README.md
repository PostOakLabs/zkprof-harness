# Measurements — 2026-08-31 · Chrome headless

This directory holds the first **real browser** runs of zkprof's pages. Before
this, every report in the repository came from Node (`tools/smoke_*.mjs`),
which is not a browser: it has no JIT warm-up profile worth quoting, no
cross-origin isolation, and a clock that resolves far finer than any browser.

## ⛔ These are single-machine, single-session, single-sample measurements. Not a benchmark.

- One sample per file, taken with the exact command and environment in
  `2026-08-31-chrome/ENVIRONMENT.md`.
- Nothing here is a published result, a claim about any third-party system, or
  a repeatable benchmark. Do not compare rows across files, machines, or
  sessions; do not quote any number as "the" value.
- **Every number here is JIT-warm.** The workload runs a warm-up round, a timed
  sizing round, a correctness-gate prove, and only then the reported run
  (`scale.rs`, documented as "The first call is a warm-up and is not timed"),
  so `3101.1 / 2970.3 / 4584.8 / 2951.2` are all fully warm. There is no cold
  figure here and no cold-versus-warm comparison; a cold first call would be
  slower (see the 2026-09-01 verify-receipt row, which records cold vs warm).
- The same workload under the same mode is **not** a benchmark repeat: the
  no-isolation scalar harness run (`4584.8 ms`) and the isolated one
  (`3101.1 ms`) are different isolation modes, so they are **not** the same
  mode. There is no same-mode repeat in this directory (4 files = 2 profiles ×
  2 isolation modes, `iterations: 1` each), so "normal run-to-run / machine-state
  variance" has no same-mode sample to rest on here — it is a caveat, not a
  measured dispersion. The 2026-08-31 run and the 2026-09-01 verify-receipt run
  are separate datasets and are not a repeat pair.

## What each directory/file is

| File | Page | Output |
|---|---|---|
| `harness-*.json` | `/harness/` | one `groth16-bn254-merkle-poseidon-d64` proving report (scalar or `+simd128`) |
| `marshal-*.json` | `/marshalling-bench/` | a table of the crossing-bench workloads plus the last workload's full report |

This directory holds the raw numbers only. Their reading (the scalar-vs-`+simd128`
delta and the marshalling overhead) is kept separate from them.
