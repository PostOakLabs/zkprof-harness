# zkprof-marshal

The JS↔WASM boundary microbench for [zkprof-web](../../README.md).

This crate answers one question: **what does it cost to cross the boundary
between JavaScript and WebAssembly, and how much of that cost goes away if you
batch across it?**

## Why it exists

Every other row in a `zkprof-report` is compute: multi-scalar multiplications,
NTTs, hashes, field arithmetic. In a browser there is a cost that is not
compute, and that no compute profiler can see — the crossing itself. A call
into wasm is not free, and a call that performs one small hash pays the whole
crossing to move 64 bytes. Call it once per hash and the crossing dominates;
batch 32 hashes into one call and it amortises across all of them.

The bench measures that directly, in the shape the effect was published in:
one Poseidon call per crossing, then batched ×8, ×32 and ×128, against a
pure-copy baseline that crosses the same number of bytes doing nothing.

## What it measures

| Shape | Payload | Work per crossing |
| --- | --- | --- |
| `poseidon` ×1 | 64 B | one Poseidon-2 compression |
| `poseidon` ×8 | 512 B | eight compressions |
| `poseidon` ×32 | 2 KiB | 32 compressions |
| `poseidon` ×128 | 8 KiB | 128 compressions |
| `copy` ×64 B | 64 B | none — sum the bytes, return 8 B |
| `copy` ×512 B, ×2 KiB, ×8 KiB | matched to the Poseidon payloads | none |

The copy payload is set to `batch × 64` bytes so that bytes-per-hash is
constant across batch sizes. That means the per-hash column compares like with
like: what changes between rows is the number of crossings, not the amount of
data.

## How it measures it

A Rust loop cannot measure the boundary, because a Rust loop does not cross
it. **The host drives.** JavaScript owns the loop, calls `marshal_call` once
per iteration, and times the loop; Rust times only the work it does on its
side of the crossing.

That split is why `zkprof-core`'s `Profiler` grew `set_region_ms` and
`record`: the profiler's clock sees a strict subset of the region the host
timed. The attribution rule is

```text
marshalling = host_total − shape_work
```

so `marshalling` holds everything the host saw that the profiler cannot
account for — the crossing, the argument encoding, and any overhead the host
loop itself paid.

One deliberate asymmetry: Poseidon shapes are spanned (accepting a small
bias) so the report carries a real `hash` row, while the copy baseline is
spanned not at all, because any clock read inside wasm would inflate the exact
boundary that shape exists to measure.

## What it emits

One schema-valid `zkprof-report` per workload, each carrying a `marshalling`
row. The batch size lives in the report's `workload_id`
(`marshalling-poseidon-batch-32`) rather than in a dedicated field: the
schema's `primitives[].name` is a closed enum with a single `marshalling`
variant, so there is no `marshalling-batch-8` row to put it in and no room to
add one. Decomposition per batch size therefore means one report per batch
size — which also makes `zkprof diff` usable on any pair of them.

`marshal_report` returns the report plus a `host::Derived` block holding the
per-call and per-hash figures. Those are division, not measurement, and they
are computed here so a page and a CLI cannot print different answers from the
same report.

## What this crate does not contain

**No results.** Every number this crate produces comes from a run on the
machine that ran it. Nothing here states how fast a boundary crossing is, how
much batching saves, or how any of it compares to anything else — including to
the published figures that motivated building it.

## Layout

- **`shape`** — the workloads, their payload sizes, and their workload ids.
- **`bench`** — the measurement protocol: `begin`, `call`, `size`, `report`.
- **`host`** — the types the host exchanges, and the derived arithmetic.
  Target-independent and unit-tested natively.
- **`bindings`** — `wasm32`-only; thin `#[wasm_bindgen]` wrappers that own no
  logic, so that nothing load-bearing escapes the native test suite.

## License

Apache-2.0. See [LICENSE](../../LICENSE).
