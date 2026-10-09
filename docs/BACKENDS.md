# zkprof Backends

This document describes the backend abstraction layer that makes zkprof backend-neutral.

## The Trait: `ZkprofBackend`

Every proving backend implements the `ZkprofBackend` trait (defined in `crates/zkprof-core/src/backend.rs`):

```rust
pub trait ZkprofBackend: fmt::Debug + Send + Sync {
    fn canonical_circuits(&self) -> Vec<CircuitId>;
    fn prove(&self, circuit: &CircuitId, input: &[u8]) -> Result<ProofArtifact, BackendError>;
    fn span_hooks(&self) -> Vec<Span>;
    fn build_report(&self, artifact: &ProofArtifact, target: Target, workload_id: &str) -> Report;
}
```

### Key Contracts

1. **Zero behavior change for existing implementors** — The first implementor (`arkworks-groth16`) produces reports that are byte-identical whether produced through the trait or through the direct prover path (the golden test re-renders the same run through the trait layer and compares to the direct run; there is no committed fixture involved).

2. **Stubs are unmistakable** — A stub backend emits a report whose `backend_id` is `"stub"`, `version` is `"stub"`, and whose timings are all `0.0` (a placeholder). `prove()` always returns `BackendError::StubNotAProver`.

3. **No fabricated numbers** — A backend must not invent proof sizes, verification times, or security claims it did not measure.

## Implemented Backends

### 1. `arkworks-groth16` (crates/zkprof-backend-arkworks)

- **Proof system**: Groth16 over BN254
- **Circuits**: `merkle-poseidon` (configurable depth), `sha256` (configurable input bytes)
- **Canonical circuits** list the circuit ids, not constraint counts. The
  constraint count is measured at runtime and recorded in each report (see
  `crates/zkprof-backend-arkworks/src/sha256.rs`): "nothing in this crate
  predicts it — SHA-256's cost per 64-byte block is a property of the upstream
  gadget, and a number this crate invented would be worse than no number." For
  the record, `merkle-poseidon-depth64` (the only count this repo derives)
  is `243 × depth + 1` for a depth-64 tree = 15,553; the sha256 counts are NOT
  restated here because no constant, test, fixture or report produces them.
- **Span hooks**: `witness_gen`, `ntt_fft`, `field_arith`, `msm`, `other`
- **Report metadata**: `backend_id: "arkworks-groth16"`, `version: REPORT_VERSION`, `commitment_scheme: "groth16"`

### 2. `stub` (crates/zkprof-backend-stub)

- **Purpose**: Reference implementation of a non-prover backend
- **Circuits**: None (empty canonical list)
- **Behavior**: `prove()` returns `Err(BackendError::StubNotAProver)`, `build_report()` emits a report whose `backend_id` is `"stub"`, `version` is `"stub"`, and whose timings are all `0.0` (a placeholder, not a measured value — the schema makes the timing fields required, so "null" is not representable and `0.0` is what a stub writes)
- **Report metadata**: `backend_id: "stub"`, `version: "stub"`, `commitment_scheme` omitted (`None`, not `null`)

## Adding a New Backend

To add a new backend (e.g., Stwo, Ligetron, a browser-native prover):

1. Create a new crate under `crates/zkprof-backend-<name>/`
2. Implement `ZkprofBackend` for your backend type
3. Ensure `prove()` returns a `ProofArtifact` that can build a schema-valid `Report`
4. The `build_report()` method must populate `backend_id`, `version`, and `commitment_scheme`
5. Add the crate to the workspace `Cargo.toml` members list
6. Write tests that verify:
   - `canonical_circuits()` returns the expected set
   - `span_hooks()` returns named prover stages
   - Reports are byte-identical to the direct prover path (if applicable)
   - Unknown circuits return `UnsupportedCircuit`, not `StubNotAProver`

## Report Fields Added by the Backend

Every report produced through the trait carries these backend-specific fields:

| Field | Description | Example |
|-------|-------------|---------|
| `backend_id` | Human-readable backend identifier | `"arkworks-groth16"` |
| `version` | Backend implementation version | `"0.1.0"` |
| `commitment_scheme` | Polynomial commitment scheme | `"groth16"` |

These fields come directly from the `ProofArtifact` returned by `prove()`.

## Schema Integration

The `zkprof-report.schema.json` (vendored from `edi-schema`) includes the three backend fields as optional properties on the report object. They are:

```json
"backend_id": {
  "type": "string",
  "minLength": 1,
  "description": "Identity of the proving backend that produced this report."
},
"version": {
  "type": "string",
  "minLength": 1,
  "description": "Backend implementation version, if the backend exposes one."
},
"commitment_scheme": {
  "type": "string",
  "minLength": 1,
  "description": "Polynomial commitment scheme the backend uses, if known."
}
```

A stub backend omits `commitment_scheme` and sets `backend_id`/`version` to `"stub"`.

## CLI Integration

The `zkprof` CLI accepts `--backend` to select a backend:

```sh
# Use arkworks backend (the default)
cargo run --release -p zkprof-cli -- prove --backend arkworks-groth16 --target native --out report.json

# Use stub backend (for testing the harness without a prover)
cargo run --release -p zkprof-cli -- prove --backend stub --target native --out report.json
```

## Test Expectations

The golden test in `zkprof-backend-arkworks`:

```rust
#[test]
fn reports_are_byte_identical_to_the_arkworks_run() {
    // Prove the same fixture twice: once through `benchmark` directly
    // and once through the trait. The reports must be BYTE-IDENTICAL
    // on every field the trait abstracts.
}
```

This test asserts byte-for-byte equality of the JSON-serialized reports, not just structural equivalence. This guarantees that adding the trait layer introduced zero behavior change.