# zkprof-core

Timing attribution core for [zkprof-web](../../README.md).

This crate owns three things and nothing else:

- **`Profiler`** — a span API that accumulates wall time per primitive label
  into a `Report`. Spans nest; the guard form (`profiler.start(...)`) and the
  closure form (`profiler.span(...)`) both work, and both use interior
  mutability so nested spans borrow the profiler immutably.
- **`Clock`** — one trait, two implementations: `std::time::Instant` on native
  and `performance.now()` via `web-sys` on `wasm32`. Plus
  `probe_resolution()`, which measures the clock's minimum observable nonzero
  delta rather than trusting a documented figure.
- **`Report`** — the Rust transcription of `zkprof-report.schema.json`, kept
  byte-aligned with it by a test that reads the vendored schema and compares
  its `required` list and enum values against what the structs serialize.

It deliberately does **not** know about any proving system. Backends
(arkworks, etc.) live in sibling crates and call into this one.

## Timer resolution is not optional

Browsers coarsen `performance.now()`: roughly 5 µs when the page is cross-origin
isolated, roughly 100 µs when it is not. A profile taken with a 100 µs clock
over a 40 µs phase is noise, and it will *look* like a measurement.

Every report therefore carries `crossOriginIsolated` and a **measured**
`timer_resolution_us`, and [`scale::scale_iterations`] sizes the iteration count
so each measured phase spans at least 1000× the resolution. See the module docs
in `scale` for the exact rule and what happens when the target cannot be met.

## Schema pinning

`schema/zkprof-report.schema.json` is a byte-for-byte copy of the file in
`PostOakLabs/engineering-decision-schema`. Its sha256 is recorded in `schema::SCHEMA_SHA256` and
checked by the repository's tooling; do not edit it here.

## License

Apache-2.0. See [LICENSE](../../LICENSE).
