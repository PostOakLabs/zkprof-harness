#!/usr/bin/env node
// SPDX-License-Identifier: Apache-2.0
//
// Runs the marshalling wasm module outside a browser and checks what comes
// back.
//
//     node tools/smoke_marshal.mjs                  # both profiles
//     node tools/smoke_marshal.mjs --only simd
//     node tools/smoke_marshal.mjs --out target/marshal-reports
//
// # Why this exists
//
// The marshalling bench is the one workload in this repo whose correctness
// depends on the host: the loop lives in JavaScript and the wasm side only
// sees its own half of each crossing. `cargo test` can check the Rust half,
// but nothing in Rust can check that the protocol — begin, size with a probe
// batch, then run the sized loop, then report — actually closes. This closes
// it, in a few seconds and with no browser.
//
// # What it is not
//
// Not a benchmark. Node is not a browser: `performance.now()` here resolves
// far finer than any browser allows, `crossOriginIsolated` is absent, and the
// JIT profile is not one worth quoting. The table it prints is for "did the
// protocol close and are the derived figures consistent", not "how fast is a
// boundary crossing". In particular it does **not** assert that batching is
// faster — that is the measurement the tool exists to take on a real browser,
// and gating a smoke test on it would turn a machine-dependent result into a
// build failure.
//
// Requires Node 18+. Not wired into CI, for the same reason as
// `tools/smoke_wasm.mjs`: the PR gate is Rust and Python. Run it by hand
// after `python tools/build_wasm.py --module marshal`.

import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const PKG_ROOT = path.join(REPO_ROOT, "web", "marshalling-bench");

/** profile -> the `target.kind` that profile must report. */
const PROFILES = {
  scalar: { kind: "wasm-scalar", simd: false },
  simd: { kind: "wasm-simd", simd: true },
};

const PROBE_CALLS = 256; // enough to size a loop, too few to be a measurement
const MAX_MEASURED_CALLS = 20_000; // ceiling so a tiny resolution cannot ask for 10^9
const SHARE_TOLERANCE = 0.02; // duplicated from zkprof-core::model

let failures = 0;

function check(label, ok, detail) {
  const mark = ok ? "ok  " : "FAIL";
  if (!ok) failures += 1;
  console.log(`  ${mark} ${label}${detail === undefined ? "" : ` — ${detail}`}`);
}

function parseArgs(argv) {
  const args = { out: null, only: null };
  for (let i = 0; i < argv.length; i += 1) {
    const flag = argv[i];
    const next = () => {
      const value = argv[i + 1];
      if (value === undefined) throw new Error(`${flag} needs a value`);
      i += 1;
      return value;
    };
    switch (flag) {
      case "--out":
        args.out = next();
        break;
      case "--only":
        args.only = next();
        break;
      case "--help":
        console.log("usage: node tools/smoke_marshal.mjs [--out DIR] [--only scalar|simd]");
        process.exit(0);
        break;
      default:
        throw new Error(`unknown argument: ${flag}`);
    }
  }
  if (args.only !== null && !(args.only in PROFILES)) {
    throw new Error(`--only must be one of ${Object.keys(PROFILES).join(", ")}`);
  }
  return args;
}

/**
 * Instantiate one profile's module.
 *
 * The glue is `.js` and this repo has no `package.json`, so Node would parse
 * it as CommonJS and choke on its `export` statements. Copying it to a `.mjs`
 * in a temp directory is the smallest way around that which does not put a
 * Node manifest into the directory the web server serves.
 */
async function loadProfile(name, tmpDir) {
  const pkgDir = path.join(PKG_ROOT, `pkg-${name}`);
  const gluePath = path.join(pkgDir, "zkprof_marshal.js");
  const wasmPath = path.join(pkgDir, "zkprof_marshal_bg.wasm");

  for (const missing of [pkgDir, gluePath, wasmPath].filter((p) => !fs.existsSync(p))) {
    throw new Error(`${missing} is missing — run: python tools/build_wasm.py --module marshal`);
  }

  const staged = path.join(tmpDir, `zkprof-marshal-${name}.mjs`);
  fs.copyFileSync(gluePath, staged);
  const glue = await import(pathToFileURL(staged).href);
  await glue.default({ module_or_path: fs.readFileSync(wasmPath) });
  return glue;
}

/** One timed loop of `calls` crossings over a pre-filled payload. */
function timeLoop(glue, payload, calls) {
  const started = performance.now();
  let sink = 0;
  for (let i = 0; i < calls; i += 1) {
    // The return value is touched so the outbound half of the crossing is
    // part of what is timed, rather than optimised away.
    sink += glue.marshal_call(payload)[0];
  }
  const elapsed = performance.now() - started;
  return { elapsed, sink };
}

function runWorkload(glue, spec, outDir) {
  glue.marshal_begin(spec.kind, spec.param);

  const payloadLen = glue.marshal_payload_len();
  if (payloadLen !== spec.payload_len) {
    check(`${spec.workload_id}: payload_len`, false, `${payloadLen} != ${spec.payload_len}`);
    return null;
  }

  const payload = new Uint8Array(payloadLen);
  for (let i = 0; i < payloadLen; i += 1) payload[i] = (i * 31 + 7) & 0xff;

  // Probe: sized, then discarded. This is the step that decides how many
  // crossings the measured loop will make.
  const probe = timeLoop(glue, payload, PROBE_CALLS);
  const plan = JSON.parse(glue.marshal_size(PROBE_CALLS, probe.elapsed));
  const calls = Math.min(plan.iterations, MAX_MEASURED_CALLS);

  // Measured: the loop whose time lands in the report.
  const measured = timeLoop(glue, payload, calls);
  const envelope = JSON.parse(glue.marshal_report(calls, measured.elapsed));

  const { report, derived } = envelope;
  const label = spec.workload_id;

  const shareSum = report.primitives.reduce((acc, row) => acc + row.share, 0);
  const marshalling = report.primitives.find((row) => row.name === "marshalling");

  check(`${label}: workload_id`, report.workload_id === spec.workload_id, report.workload_id);
  check(`${label}: has a marshalling row`, marshalling !== undefined);
  check(`${label}: shares sum to 1`, Math.abs(shareSum - 1) <= SHARE_TOLERANCE, shareSum.toFixed(6));
  check(`${label}: total_ms > 0`, report.total_ms > 0, `${report.total_ms.toFixed(3)} ms`);
  check(`${label}: iterations`, report.iterations === calls, `${report.iterations} vs ${calls}`);
  check(`${label}: us_per_call finite`, Number.isFinite(derived.us_per_call) && derived.us_per_call > 0, derived.us_per_call.toFixed(4));

  // The per-hash figure exists exactly for the Poseidon shapes. A copy
  // baseline performs no hashes, and reporting zero there would be a claim
  // rather than an absence.
  const isPoseidon = spec.kind === "poseidon";
  check(
    `${label}: us_per_hash ${isPoseidon ? "present" : "absent"}`,
    isPoseidon ? derived.us_per_hash !== null : derived.us_per_hash === null,
    String(derived.us_per_hash),
  );
  check(`${label}: bytes_per_call`, derived.bytes_per_call === spec.payload_len, `${derived.bytes_per_call}`);
  check(`${label}: payload bytes constant per hash`, isPoseidon ? derived.bytes_per_call / derived.hashes_per_call === 64 : true, isPoseidon ? String(derived.bytes_per_call / derived.hashes_per_call) : "n/a");

  if (outDir) {
    const file = path.join(outDir, `${report.workload_id}.json`);
    fs.writeFileSync(file, `${JSON.stringify(report, null, 2)}\n`);
  }

  return { spec, report, derived, plan, calls };
}

async function runProfile(name, args, tmpDir) {
  const expected = PROFILES[name];
  console.log(`\n=== ${name} ===`);

  const glue = await loadProfile(name, tmpDir);

  check("marshal_target_kind()", glue.marshal_target_kind() === expected.kind, glue.marshal_target_kind());
  check("marshal_simd_enabled()", glue.marshal_simd_enabled() === expected.simd, String(glue.marshal_simd_enabled()));

  const target = JSON.parse(glue.marshal_target_json());
  check("marshal_target_json() kind", target.kind === expected.kind, target.kind);

  const resolution = glue.marshal_timer_resolution_us();
  check("timer resolution > 0", resolution > 0, `${resolution.toFixed(4)} µs`);

  const suite = JSON.parse(glue.marshal_suite());
  check("suite is non-empty", suite.length > 0, `${suite.length} workloads`);
  check(
    "suite alternates poseidon and copy",
    suite.every((spec, i) => (i % 2 === 0 ? spec.kind === "poseidon" : spec.kind === "copy")),
    suite.map((s) => s.kind).join(","),
  );
  check(
    "every copy baseline carries its poseidon payload",
    suite
      .filter((_, i) => i % 2 === 1)
      .every((spec, i) => spec.payload_len === suite[i * 2].payload_len),
  );

  // Both error paths are part of the contract: the host can name a shape
  // that does not exist, and can hand a payload of the wrong length.
  let refusedKind = false;
  try {
    glue.marshal_begin("sha3", 1);
  } catch {
    refusedKind = true;
  }
  check("an unknown shape is refused", refusedKind);

  glue.marshal_begin("copy", 64);
  let refusedLen = false;
  try {
    glue.marshal_call(new Uint8Array(7));
  } catch {
    refusedLen = true;
  }
  check("a wrong-length payload is refused", refusedLen);
  glue.marshal_abort();

  const outDir = args.out ? path.join(args.out, expected.kind) : null;
  if (outDir) fs.mkdirSync(outDir, { recursive: true });

  const rows = [];
  for (const spec of suite) {
    try {
      const row = runWorkload(glue, spec, outDir);
      if (row) rows.push(row);
    } catch (error) {
      failures += 1;
      console.log(`  FAIL ${spec.workload_id}: ${error && error.message ? error.message : error}`);
      glue.marshal_abort();
    }
  }

  if (rows.length === 0) return;

  console.log("");
  console.log("       workload                        calls   us/call    us/hash   bytes  marshalling%");
  for (const { spec, report, derived, calls } of rows) {
    const marshalling = report.primitives.find((row) => row.name === "marshalling");
    const perHash = derived.us_per_hash === null ? "     n/a" : derived.us_per_hash.toFixed(3).padStart(8);
    console.log(
      `       ${spec.workload_id.padEnd(28)} ${String(calls).padStart(6)} ` +
        `${derived.us_per_call.toFixed(3).padStart(9)} ${perHash} ` +
        `${String(derived.bytes_per_call).padStart(7)} ` +
        `${((marshalling?.share ?? 0) * 100).toFixed(1).padStart(10)}%`,
    );
  }

  console.log("");
  console.log(`       clock       ${resolution.toFixed(4)} µs  (Node: finer than any browser allows)`);
  console.log(`       plan       ${rows[0].plan.description}`);
  if (outDir) console.log(`       wrote      ${rows.length} report(s) to ${outDir}`);
}

async function main() {
  const args = parseArgs(process.argv.slice(2));
  const names = args.only ? [args.only] : Object.keys(PROFILES);
  const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "zkprof-marshal-"));

  try {
    for (const name of names) {
      // A failure in one profile should not hide the other.
      try {
        await runProfile(name, args, tmpDir);
      } catch (error) {
        failures += 1;
        console.log(`\n=== ${name} ===\n  FAIL ${error && error.message ? error.message : error}`);
      }
    }
  } finally {
    fs.rmSync(tmpDir, { recursive: true, force: true });
  }

  console.log("");
  if (failures > 0) {
    console.log(`${failures} check(s) failed.`);
    process.exitCode = 1;
    return;
  }
  console.log("all checks passed.");
  console.log("The table above is a protocol check, not a measurement: Node is not a browser.");
  console.log("To take the real measurement, serve the page and open it:");
  console.log("  python web/server.py");
}

try {
  await main();
} catch (error) {
  console.error(`error: ${error && error.message ? error.message : error}`);
  process.exitCode = 1;
}
