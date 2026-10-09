# zkprof-cli

The `zkprof` binary.

```sh
zkprof prove --backend arkworks-groth16 --target native --out report.json
zkprof render report.json
zkprof diff base.json compare.json
```

## `prove`

| flag | default | meaning |
|------|---------|---------|
| `--backend` | `arkworks-groth16` | which implementation to run |
| `--target` | `native` | `native`, `wasm-scalar`, `wasm-simd`, `wasm-threads`, `webgpu` |
| `--out` | required | where to write the report |
| `--depth` | `64` | Merkle depth; `243 * depth + 1` constraints |
| `--seed` | `0x5EED` | seed for witness, setup and prover randomness |
| `--workload-id` | derived from depth | the `workload_id` recorded in the report |

Only `--target native` is built so far. The other four are accepted and
refused with a reason, which is the only honest response: filling in a
`wasm-simd` report from a native run would be fabricating a measurement.

## `render`

```sh
zkprof render report.json
```

Reads a `zkprof-report` document and prints the same per-primitive table
`prove` prints, but from a saved document instead of a fresh measurement.
Per-operation figures divide the reported totals by the report's own
`iterations`, so the numbers match what `prove` would have shown for that run.

`render` refuses a document that does not validate against the vendored
zkprof-report schema (shares must partition the primitive time, `workload_id`
must be non-empty, the clock resolution must be finite and non-negative, and so
on). A table built from a malformed document would present noise as a
measurement, so the tool errors out instead.

## `diff`

```sh
zkprof diff base.json compare.json
```

Reads two `zkprof-report` documents and prints a per-primitive delta table:
`Δ ms/op`, the same difference as a percentage of the baseline `ms/op`, and the
change in share. Rows present in only one report are still shown, marked with a
`+` (appeared in the comparand) or `-` (disappeared from the baseline) suffix,
because a missing primitive is a real change, not something to hide.

Both inputs must validate against the vendored schema; a delta against a
malformed report is meaningless, so the tool refuses.

## Printed versus written

The table prints **per-iteration** figures; the JSON holds **totals** for the
run. With one iteration — the usual case for a proof this size — they are the
same number. Per-iteration is what makes two runs comparable; totals are what
make the JSON a faithful record of what happened.

Rows whose accumulated time is under 1000× the measured clock resolution are
marked `<- below target` rather than quietly published.

## License

Apache-2.0. See [LICENSE](../../LICENSE).
