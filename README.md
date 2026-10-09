# zkprof-web

Browser-native profiling harness for zero-knowledge proving workloads.

The question this repository exists to answer is not "how fast is Groth16 in a
browser" but **"where does the time go, and can that answer be trusted?"** A
browser profile is only as good as the clock that took it, and browsers
deliberately coarsen `performance.now()` unless the page is cross-origin
isolated. So every artifact this repository produces carries the clock's
measured resolution and the isolation state it was measured under, and the
harness refuses to report a number it could not actually resolve.

## Status

Research prototype. Every work unit in the table below is merged to `main`.
Nothing in this repository is a benchmark (see Results).

| Work unit | What it is | State |
|-----------|------------|-------|
| W3.1 | `zkprof-core`: span API, clock abstraction, timer-resolution handling, report model | in place |
| W3.2 | `zkprof-backend-arkworks` + `zkprof prove`: real Groth16/BN254 workload on the native target | in place |
| W3.3 | WASM targets (scalar and `+simd128`), harness page, COOP/COEP server | in place |
| W3.4 | Marshalling microbenchmark across the host/WASM boundary | in place |
| W3.5 | WebGPU plumbing verification: M31 kernel dispatched, checked against the CPU reference | in place |
| W3.6 | `zkprof render` and `zkprof diff` | in place |
| W3.7 | CI gate and fixtures | in place |
| W3.8 | csp-benchmarks exporter, and the `sha256` canonical circuit to export | in place |
| W3.9 | WebGPU timestamp-query timing | in place |
| W3.10 | Backend-neutral harness: `ZkprofBackend` trait + arkworks impl + stub backend | in place |
| W3.11 | FibRace-compatible reporting (`fibrace-view`) | in place |
| W3.12 | WebGPU crate README (`crates/zkprof-webgpu/README.md`) | in place |
| W3.13 | AINumbers workload characterization one-pager (`docs/AINUMBERS-WORKLOAD.md`) | in place |
| RV1–RV4 | Receipt verifier: `zkprof-verify` crate, journal-bytes hypothesis, wasm verify page, snarkjs twin | in place |
| RV5 | Combined receipt-bundle verifier: one command, proof half (twin) + transparency half (independent Sigsum/C2SP reimplementation), verdicts never collapsed ([docs/BUNDLE-VERIFIER.md](docs/BUNDLE-VERIFIER.md)) | in place |

The report schema version is `REPORT_VERSION` (`0.1.0`), not `v0.3.0`; `schema v0.3.0` was a stale description in the W3.10 row.

### W3.10 Backend Selection

| `--backend` | Crate | Circuits | Status |
|-------------|-------|----------|--------|
| `arkworks-groth16` | `zkprof-backend-arkworks` | merkle-poseidon, sha256 | production |
| `stub` | `zkprof-backend-stub` | (none) | test harness |

See [BACKENDS.md](docs/BACKENDS.md) for full backend contract and adding new backends.

## Results

The repository's own proving workloads are measured and recorded, but nothing
here is a comparative result: there is no claim that any backend beats any
other. The committed artifacts (`measurements/`) are single-machine,
single-session, single-sample runs — explicitly *not* benchmarks (see the
per-directory READMEs). See `docs/AINUMBERS-WORKLOAD.md` for the AINumbers
workload characterization; the receipt-verifier browser measurement is in
`measurements/2026-09-01-verify-receipt/`.

## Layout

```
Cargo.toml                     workspace root: shared deps and lint policy
crates/
  zkprof-core/                 span API, clocks, report model, vendored schema
    schema/                    byte-for-byte copy of edi-schema's report schema
  zkprof-backend-arkworks/     Groth16/BN254: circuit, span-split prover, driver
  zkprof-wasm/                 the browser surface: wasm-bindgen entry point
  zkprof-marshal/              the JS↔WASM boundary microbench
  zkprof-cli/                  the `zkprof` binary
tools/
  check_vendored_schema.py     verifies the vendored copy against its pinned hash
  build_wasm.py                builds both wasm modules in each profile
  smoke_wasm.mjs               runs the harness modules outside a browser
  smoke_marshal.mjs            runs the marshalling protocol end to end
  bin/                         the prebuilt wasm-bindgen CLI lands here (gitignored)
web/
  server.py                    dev server, COOP/COEP by default; serves web/
  index.html                   landing page; measures nothing
  shared.css                   chrome for both pages
  harness/                     the profiling page + pkg-<profile>/ (gitignored)
  marshalling-bench/           the boundary bench page + pkg-<profile>/ (gitignored)
```

## Quick start

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
cargo clippy --workspace --target wasm32-unknown-unknown -- -D warnings
python tools/check_vendored_schema.py
```

Running a workload — this writes a report but publishes nothing:

```sh
cargo run --release -p zkprof-cli -- prove --backend arkworks-groth16 \
    --target native --out report.json
```

`--target` accepts the other four spellings the report schema allows
(`wasm-scalar`, `wasm-simd`, `wasm-threads`, `webgpu`). This binary is natively
compiled, so it refuses all four and says where each one actually lives: the
wasm targets run in the browser harness below, `wasm-threads` builds but
refuses to produce a report, and `webgpu` is W3.5. Asking for a target that
does not exist gives an error, not a report with a target field filled in from
a native run.

## Running the browser harness

The same backend and the same workload as `zkprof prove`, compiled to
`wasm32-unknown-unknown` in two profiles — scalar and `+simd128` — and run in
your own browser.

```sh
python tools/build_wasm.py        # both modules, both profiles
python web/server.py              # http://127.0.0.1:8737/
```

The server serves `web/` and prints the URL of every page it finds, so the
same command reaches both the harness and the [marshalling
bench](#the-marshalling-microbench).

The script needs the wasm-bindgen CLI at the version `Cargo.lock` pins, and
refuses to build if it does not match: the generated glue and the Rust side
share a binary protocol, and a mismatch fails at instantiation with an error
that names neither the tool nor the version. `cargo install wasm-bindgen-cli`
needs `dlltool` on Windows, so the script expects a prebuilt release instead:

```sh
python tools/build_wasm.py --print-download-info   # exact asset and sha256
```

Then point a browser at the page, pick a profile, and press Run. The workload
runs in a Web Worker, so the tab stays responsive; the report is rendered
on-page and downloadable as schema-valid JSON.

To check the modules without a browser — both profiles instantiate, report the
target they were compiled for, and produce a report whose primitive shares sum
to 1:

```sh
node tools/smoke_wasm.mjs
```

### Both deployment contexts, on purpose

Browsers coarsen `performance.now()` to blunt timing side channels, and the
coarsening depends on whether the page is cross-origin isolated: roughly 5 µs
when it is, roughly 100 µs when it is not. That is a 20× difference in the
resolution of every number this project produces, so the server's default is
to send the two headers that turn isolation on:

```
Cross-Origin-Opener-Policy: same-origin
Cross-Origin-Embedder-Policy: require-corp
```

and `--no-isolation` serves the same directory without them. Both contexts
have to be runnable, because the constraint this harness exists to measure
includes the un-isolated case. The page shows which one it is in, and the
report records `crossOriginIsolated` alongside the measured clock resolution.

### What the wasm target reports about itself

| field | where it comes from |
|-------|---------------------|
| `target.kind` | `cfg!` at compile time — the module cannot misreport which profile it is |
| `target.browser`, `target.os` | `navigator.userAgent` and `navigator.platform`, read as-is. Not parsed: a report that says `os: linux` because a parser recognised a token is reporting an inference |
| `target.cpu` | `wasm32`, with ` +simd128` appended when the profile has it |
| `memory.peak_heap_bytes` | the size of the module's linear memory after the run |

That last one is narrower than it looks, and the narrowness is deliberate. It
answers "how big did linear memory get", not "how much did this workload
allocate": the default Rust allocator on wasm32 grows linear memory and does
not shrink it, so the figure is a module-wide high-water mark that includes
everything allocated before the run started. It is still a real observation,
and it is the only memory figure a wasm module can read about itself. Off
`wasm32` the field is absent rather than zero.

### Threads: plumbed, not implemented

`--features web-threads` builds a module that reports `target.kind:
wasm-threads` and refuses every `run_benchmark` call with a message saying so.
The flag exists so the build plumbing is wired and the unimplemented path is
one grep away — not so that the feature can be used. A report produced under a
threads flag nobody implemented would look exactly like a real one.

## What the W3.2 workload is

`zkprof prove` runs a Groth16 proof over BN254. There are two circuits to
choose from, and the choice is the workload:

| `--circuit` | what it proves | size knob | default |
|-------------|----------------|-----------|---------|
| `merkle-poseidon` | membership of a leaf in a Poseidon Merkle tree, against a public root | `--depth`, one Poseidon permutation per level | depth 64, 15,553 constraints (`243 * depth + 1`; the constant is derived in `zkprof-backend-arkworks` and checked by a test at several depths) |
| `sha256` | knowledge of an N-byte preimage of a public 32-byte digest | `--bytes`, one of 128/256/512/1024/2048 | 128 bytes |

`sha256` is a canonical circuit from csp-benchmarks, which is the point of
having it: a number measured on it can be compared with that suite, and the
same circuit run natively and in a browser is the cross-target delta in one
row. Merkle-Poseidon is this repository's own shape and is comparable with
nothing outside it.

The exact constraint count is measured at run time in both cases — never
predicted from the depth or the input size — and recorded in each report as
`system.constraint_count`.

The prover is `ark-groth16`'s, taken apart so each stage can be timed:

| primitive | what it covers |
|-----------|----------------|
| `witness_gen` | circuit synthesis, LC inlining, sparse matrix construction, witness evaluation — one span, because arkworks exposes one call |
| `ntt_fft` | the R1CS-to-QAP witness map: three inverse FFTs, two forward FFTs over a coset domain, and the polynomial multiply/divide between them |
| `field_arith` | witness scalars out of Montgomery form into the big-integer form the MSM wants |
| `msm` | the five multi-scalar multiplications: `h`, `l`, `a`, `b` in G1, `b` in G2 |
| `other` | scalar multiplications by `r` and `s`, the group operations assembling C, the affine conversions |

Two things about that table are worth saying out loud:

- **Poseidon is not under `hash`.** The permutations that dominate witness
  generation run inside `generate_constraints`, which has no hook to subdivide
  it. Attributing them to `hash` would mean patching arkworks; attributing
  them to `witness_gen` is what the call as a whole computes.
- **`other` is not a junk drawer.** It is a real, measured cost. It is simply
  not one of the four named primitives.

Taking a prover apart risks changing its answer, so `prove_with_spans` is
tested against `Groth16::create_proof_with_reduction` with the same key and
the same randomness: `matches_the_upstream_proof` asserts the two proofs are
equal. Every run also verifies its own proof before it will emit a report.

Setup and verification are outside the measured region. Setup is a one-off per
circuit; including it would make `total_ms` a property of the setup rather than
of the workload.

## The marshalling microbench

Every other row in a `zkprof-report` is compute: multi-scalar multiplications,
NTTs, hashes, field arithmetic. In a browser there is a fourth cost that is
not compute, and that no compute profiler can see — **the crossing itself**. A
call into wasm is not free, and a call that performs one small hash pays the
whole crossing to move 64 bytes. The measured marshalling share across the four
committed runs is 1.9–3.8% of a one-hash call — a real, non-zero cost, but it
does **not** dominate a single call, and batching hashes into one call amortises
the crossing across them (µs/hash stays roughly flat, at ~92–98 µs, rather
than falling). The earlier "the crossing dominates" phrasing was contradicted by
the run's own committed data; this is the corrected statement.

This is the tool's signature row, so here is exactly what it measures and how.

### The shape

One Poseidon compression per boundary crossing, then batched ×8, ×32 and
×128, against a pure-copy baseline that crosses the same bytes doing nothing:

| shape | payload per call | work per call |
|-------|------------------|---------------|
| `poseidon` ×1 | 64 B | one 2:1 Poseidon compression |
| `poseidon` ×8 | 512 B | eight compressions |
| `poseidon` ×32 | 2 KiB | 32 compressions |
| `poseidon` ×128 | 8 KiB | 128 compressions |
| `copy` ×64 B … ×8 KiB | matched to the Poseidon payloads | none — sum the bytes, return 8 B |

The copy payload is `batch × 64` bytes, so bytes-per-hash is constant across
batch sizes. That is what makes the per-hash column comparable: what changes
between rows is the number of crossings, not the amount of data.

### How it is measured

**The loop is in JavaScript, not in Rust.** A Rust loop never crosses the
boundary, so a Rust loop cannot measure it. The page owns the loop, calls into
wasm once per iteration, and times the loop; the wasm side times only the work
it does on its own side of the crossing.

That split is why `Profiler` grew `set_region_ms` and `record`: the profiler's
clock sees a strict subset of the region the host timed. The attribution rule
is

```
marshalling = host_total − shape_work
```

so `marshalling` holds everything the host saw that the profiler cannot account
for — the crossing, the argument encoding, and the host loop's own overhead.

One deliberate asymmetry: Poseidon shapes are spanned, so the report carries a
real `hash` row, while the copy baseline is spanned not at all. Any clock read
inside wasm would inflate the exact boundary the baseline exists to measure.

### Why one report per batch size

The batch size is in the report's `workload_id`
(`marshalling-poseidon-batch-32`), not in a field of its own, because the
schema's `primitives[].name` is a closed enum with a single `marshalling`
variant: there is no `marshalling-batch-8` row to put it in and no room to add
one. Decomposition per batch size therefore means one report per batch size —
which also makes `zkprof diff` usable on any pair of them (W3.6).

### Running it

```sh
python tools/build_wasm.py --module marshal
python web/server.py            # then open /marshalling-bench/
```

or without a browser, to check that the protocol closes end to end:

```sh
node tools/smoke_marshal.mjs
```

The smoke script prints the per-batch table but deliberately does **not** gate
on batching being faster. That is the measurement this tool exists to take on
a real browser, and Node's clock and JIT are not that — gating on it would
turn a machine-dependent result into a build failure.

See [`crates/zkprof-marshal/README.md`](crates/zkprof-marshal/README.md) for
the crate-level detail.

## The three rules this repository measures by

1. **Measure the clock, never assume it.** `zkprof-core` probes
   `performance.now()` for its minimum observable nonzero delta rather than
   trusting a documented figure, because the coarsening depends on the
   deployment, not the code.
2. **Scale the workload to the clock.** Each measured phase must span at least
   1000× the measured resolution. When that is impossible, the harness says so
   instead of publishing a number that is really noise.
3. **Do not invent attribution.** Time outside every span is reported as
   unattributed, not folded into the `other` bucket. A large `other` share is a
   statement about instrumentation coverage, not about performance.

## Timing the WebGPU dispatch

W3.5 put an M31 kernel on the GPU; W3.9 measures it. A run reports **two**
figures for the same dispatch whenever the adapter allows it, because they are
not two estimates of one quantity — they are measurements of two different
ones:

| phase | instrument | what it spans |
| --- | --- | --- |
| `dispatch` | host clock | submission to completion — queue latency and submit overhead included |
| `dispatch-gpu-pass` | `GPUQuerySet` timestamps | the compute pass, start to end |

Which instrument produced a figure is not left to inference. The report carries
`gpu_timing_source` — `"timestamp-query"` or `"wall-clock"` — an optional field
in the schema `edi-schema` owns, added there as W1.2b (release 0.2.1) and
vendored here byte-for-byte.

**The timestamp-query figure is pass-level only; copies/resource-creation/submit
overhead are invisible to it (that is precisely why both numbers ship).**

That sentence is the reason the second number was added instead of replacing
the first. The pass figure answers "how long did the device spend on this
pass"; the wall clock answers "how long did this dispatch cost the host". The
gap between them *is* the overhead, and publishing only the narrower number
would read as a speedup that never happened. The pass figure is narrower, not
better.

`timestamp-query` is optional in WebGPU and not universally available. The run
feature-detects it on the adapter, requests it on the device, and falls back to
the single wall-clock figure when either says no — and the report then says
which instrument it used, rather than leaving the absence to be guessed at.

Three behaviours here were established by reading wgpu 30 rather than its
documentation, and are recorded because each one is a trap:

- `ComputePass::write_timestamp` **panics on the browser backend** —
  write-timestamp-inside-a-pass was removed from the WebGPU spec (gpuweb#4370)
  and wgpu kept the call for native backends only. Only the descriptor form
  (`ComputePassDescriptor::timestamp_writes`) is used here, because it is the
  only one the browser implements.
- `MAP_READ` is legal alongside `COPY_DST` and never with `QUERY_RESOLVE`, so
  resolve and readback are two buffers and one copy.
- `queue.get_timestamp_period()` is 1.0 in the browser and has been coarser on
  native backends, so it is read rather than assumed.

The counter arithmetic lives in `crates/zkprof-webgpu/src/timing.rs` and is
deliberately native code, so that it can be unit-tested — counter wrap, the
unwritten-query case, and a non-positive period all fail a test rather than a
browser session. `pipeline.rs` cannot be reached by a unit test; this module
can.

This is plumbing plus an instrument. No performance claim is made about either
figure, and no report in this repository carries one.

## The csp-benchmarks adapter

`zkprof export --format csp-benchmarks` converts reports into the result shape
[`csp-benchmarks`](https://github.com/ethereum/csp-benchmarks) (PSE) publishes
quarterly on ethproofs.org. The exporter is only useful because
`--circuit sha256` can measure one of that suite's canonical circuits, and
which canonical circuits this repository *cannot* measure is documented there
rather than approximated here.

Fields a zkprof report cannot supply are omitted from the exported JSON rather
than emitted as 0 — a zero in `proof_size` claims the proof is empty. No
submission to ethproofs has been made from this repository; export capability
is where it stops.

## Related work

- [`csp-benchmarks`](https://github.com/ethereum/csp-benchmarks) — PSE's
  client-side-proving benchmark harness: canonical circuits, results published
  quarterly on ethproofs.org. The source of the `sha256` circuit W3.8 adds.
- [FibRace](https://arxiv.org/pdf/2510.14693) — large-scale client-side proving
  on mobile hardware.

## Relationship to edi-schema

Reports produced here validate against `zkprof-report.schema.json`, which is
owned by [`PostOakLabs/engineering-decision-schema`](https://github.com/PostOakLabs/engineering-decision-schema)
and vendored (byte-for-byte, hash-pinned) into `crates/zkprof-core/schema/`.
`tools/check_vendored_schema.py` catches drift.

## License

Apache-2.0. See [LICENSE](LICENSE).
