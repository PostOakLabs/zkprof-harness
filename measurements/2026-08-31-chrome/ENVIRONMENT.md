# Environment — first browser measurement run (2026-08-31)

Single machine, single session, headless browser. **Not a benchmark.** Every
number here is one sample taken with the exact command and environment recorded
below. Different machine, load, or clock state will differ. Read
`2026-08-31-chrome/README.md` (this directory) and the caveat there before
treating any of it as data.

## Machine

| | |
|---|---|
| OS | Microsoft Windows 11 Home, 10.0.26200 build 26200, 64-bit |
| CPU | Intel Core i9-10850K @ 3.60 GHz — 10 cores, 20 logical processors |
| GPU | NVIDIA GeForce RTX 3080 (driver 32.0.15.9595, 4 GB) + Parsec Virtual Display Adapter |
| Browser | Google Chrome 151.0.7922.175, `--headless=new` (headless Chrome). Edge 152.0.4191.53 was present but not used; Chrome is the primary. |
| Toolchain | Rust/cargo 1.98.0 (stable-x86_64-pc-windows-gnu), wasm32-unknown-unknown target, wasm-bindgen CLI 0.2.127 |
| Repo commit | NOT RECORDED in the original run. These artifacts were produced on 2026-08-31 against a `main` that predates `backend_id`/`version`/`commitment_scheme`, so a re-run today emits a different report shape with no note on file. The cargo profile was `--release` (the build was run via `tools/build_wasm.py` default, which is release); that is the only profile datum recorded. |
| Server | `python web/server.py` (stdlib) — served `web/` |

## Server modes

| Mode | Command | COOP / COEP | Clock |
|---|---|---|---|
| isolated | `python web/server.py` | `same-origin` / `require-corp` | fine (~5 µs) |
| no-isolation | `python web/server.py --port 8738 --no-isolation` | none | coarsened (~100 µs) |

| Page | Source | Needs wasm build? | How the run was started |
|---|---|---|---|
| `/harness/` | on `main` | yes (pkg-scalar, pkg-simd) | CDP: select profile, click Run |
| `/marshalling-bench/` | on `main` | yes (pkg-scalar, pkg-simd) | CDP: select profile, click Run, dump table |

## Report files and how each was produced

| File | Server mode | Profile | UTC (approx) |
|---|---|---|---|
| `harness-isolated-scalar.json` | isolated | scalar (`wasm-scalar`) | 2026-08-31 |
| `harness-isolated-simd.json` | isolated | `+simd128` (`wasm-simd`) | 2026-08-31 |
| `harness-noisolation-scalar.json` | no-isolation | scalar | 2026-08-31 |
| `harness-noisolation-simd.json` | no-isolation | `+simd128` | 2026-08-31 |
| `marshal-isolated-scalar.json` | isolated | scalar | 2026-08-31 |
| `marshal-isolated-simd.json` | isolated | `+simd128` | 2026-08-31 |
| `marshal-noisolation-scalar.json` | no-isolation | scalar | 2026-08-31 |
| `marshal-noisolation-simd.json` | no-isolation | `+simd128` | 2026-08-31 |

## How the browser was driven

Headless Chrome with a DevTools Protocol socket (remote debugging on
`127.0.0.1:9222`), driven by a small dependency-free Node script using Node's
built-in WebSocket. No page source was modified for the harness or
marshalling-bench measurements (the run was triggered via CDP; the report JSON
was read from the page's existing raw-JSON element). This run therefore needed
**no `?autorun=1` page change** — no `browser-run-1/autorun` branch was created.

The wasm profiles were rebuilt immediately before serving on this branch with
`python tools/build_wasm.py` (exit 0, all scalar + simd). Run on 2026-08-31 in
UTC.
