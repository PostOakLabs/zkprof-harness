# zkprof-webgpu

WebGPU plumbing verification for [zkprof-web](../../README.md).

This crate is an **experimental / stub-status** crate. It dispatches a single
M31 multiplication kernel on WebGPU, reads the result back, and checks it
against a CPU reference. It exists to verify that the WebGPU path works — that a
kernel launches, a `GPUQuerySet` timestamp can be resolved, and the result
round-trips — **not** to report how fast any of it is.

**Contains no performance claims.** No number in this crate is a benchmark, and
nothing here is compared to any other implementation. The only figure of note is
the pass-time instrumentation gap that W3.9 documents: the timestamp-query
figure covers the pass only; buffer copies, resource creation and submission
latency are invisible to it (which is why a report carrying it also carries a
host-clock figure for the same region).

Status: experimental wiring, not a stable public surface. The WebGPU path is
plumbed and verified against a trivial kernel; it is not the workload the
harness measures (the harness runs the arkworks Groth16 backend in wasm). A
subsequent backend that targets WebGPU would be the first real consumer of this
crate.

Build with the rest of the workspace; the WebGPU feature is gated and does not
affect the harness.
