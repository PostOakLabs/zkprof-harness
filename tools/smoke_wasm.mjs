#!/usr/bin/env node
// SPDX-License-Identifier: Apache-2.0
//
// Runs the built wasm modules outside a browser and checks what comes back.
//
//     node tools/smoke_wasm.mjs                 # both profiles, depth 8
//     node tools/smoke_wasm.mjs --depth 32
//     node tools/smoke_wasm.mjs --out /tmp/reports
//
// # Why this exists
//
// The wasm module is the one artefact in this repo that nothing else can
// exercise: `cargo test` builds it for the host, and the harness page needs a
// human with a browser. This closes the gap. It answers, in a few seconds and
// with no browser, whether both profiles still build, instantiate, report the
// target they were compiled for, and produce a report whose primitive shares
// add up.
//
// # What it is not
//
// Not a benchmark. Node is not a browser: `performance.now()` here resolves
// far finer than any browser allows, `crossOriginIsolated` is absent, and
// there is no JIT warm-up profile worth quoting. The numbers it prints are
// for "did that work", not "how fast is it".
//
// Requires Node 18+. Not wired into CI: the PR gate is Rust and Python, and
// adding a Node job for a developer convenience script is not obviously a
// good trade. Run it by hand after `python tools/build_wasm.py`.

import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const PKG_ROOT = path.join(REPO_ROOT, "web", "harness");

/** profile -> the `target.kind` that profile must report. */
const PROFILES = {
  scalar: { kind: "wasm-scalar", simd: false },
  simd: { kind: "wasm-simd", simd: true },
};

const DEFAULT_DEPTH = 8;
const DEFAULT_SEED = 24237n; // 0x5EED, matching the crate's DEFAULT_SEED
const SHARE_TOLERANCE = 0.02; // duplicated from zkprof-core::model

let failures = 0;

function check(label, ok, detail) {
  const mark = ok ? "ok  " : "FAIL";
  if (!ok) failures += 1;
  console.log(`  ${mark} ${label}${detail === undefined ? "" : ` — ${detail}`}`);
}

function parseArgs(argv) {
  const args = { depth: DEFAULT_DEPTH, seed: DEFAULT_SEED, out: null, only: null };
  for (let i = 0; i < argv.length; i += 1) {
    const flag = argv[i];
    const next = () => {
      const value = argv[i + 1];
      if (value === undefined) throw new Error(`${flag} needs a value`);
      i += 1;
      return value;
    };
    switch (flag) {
      case "--depth":
        args.depth = Number(next());
        break;
      case "--seed":
        args.seed = BigInt(next());
        break;
      case "--out":
        args.out = next();
        break;
      case "--only":
        args.only = next();
        break;
      case "--help":
        console.log("usage: node tools/smoke_wasm.mjs [--depth N] [--seed N] [--out DIR] [--only scalar|simd]");
        process.exit(0);
        break;
      default:
        throw new Error(`unknown argument: ${flag}`);
    }
  }
  if (!Number.isInteger(args.depth) || args.depth < 1) {
    throw new Error(`--depth must be a positive integer, got ${args.depth}`);
  }
  if (args.only !== null && !(args.only in PROFILES)) {
    throw new Error(`--only must be one of ${Object.keys(PROFILES).join(", ")}`);
  }
  return args;
}

/**
 * Instantiate one profile's module.
 *
 * The glue is `.js`, and this repo has no `package.json`, so Node would parse
 * it as CommonJS and choke on its `export` statements. Copying it to a `.mjs`
 * in a temp directory is the smallest way around that which does not put a
 * Node manifest into the directory the web server serves.
 *
 * The wasm bytes are passed explicitly rather than left to the glue's
 * `import.meta.url` resolution, which would also work but for a temp path
 * that has nothing to do with the real output directory.
 */
async function loadProfile(name, tmpDir) {
  const pkgDir = path.join(PKG_ROOT, `pkg-${name}`);
  const gluePath = path.join(pkgDir, "zkprof_wasm.js");
  const wasmPath = path.join(pkgDir, "zkprof_wasm_bg.wasm");

  for (const missing of [pkgDir, gluePath, wasmPath].filter((p) => !fs.existsSync(p))) {
    throw new Error(`${missing} is missing — run: python tools/build_wasm.py`);
  }

  const staged = path.join(tmpDir, `zkprof-wasm-${name}.mjs`);
  fs.copyFileSync(gluePath, staged);
  const glue = await import(pathToFileURL(staged).href);
  await glue.default({ module_or_path: fs.readFileSync(wasmPath) });
  return glue;
}

function summarise(report) {
  return report.primitives
    .map((row) => `${row.name}=${(row.share * 100).toFixed(1)}%`)
    .join("  ");
}

async function runProfile(name, args, tmpDir) {
  const expected = PROFILES[name];
  console.log(`\n=== ${name} ===`);

  const glue = await loadProfile(name, tmpDir);

  check("target_kind()", glue.target_kind() === expected.kind, glue.target_kind());
  check("simd_enabled()", glue.simd_enabled() === expected.simd, String(glue.simd_enabled()));
  check("threads_enabled()", glue.threads_enabled() === false, String(glue.threads_enabled()));

  const target = JSON.parse(glue.target_json());
  check("target_json() kind", target.kind === expected.kind, target.kind);
  console.log(`       target: ${JSON.stringify(target)}`);

  // A zero depth must be refused at the boundary, not silently defaulted.
  let refused = false;
  try {
    glue.run_benchmark(0, args.seed);
  } catch {
    refused = true;
  }
  check("depth 0 is refused", refused);

  const started = performance.now();
  const json = glue.run_benchmark(args.depth, args.seed);
  const wallMs = performance.now() - started;

  let envelope;
  try {
    envelope = JSON.parse(json);
  } catch {
    check("run_benchmark returns JSON", false, `${json.slice(0, 120)}`);
    return;
  }
  check("run_benchmark returns JSON", true);

  const report = envelope.report;
  const shareSum = report.primitives.reduce((acc, row) => acc + row.share, 0);
  check("primitive shares sum to 1", Math.abs(shareSum - 1) <= SHARE_TOLERANCE, shareSum.toFixed(6));
  check("total_ms is positive", report.total_ms > 0, `${report.total_ms.toFixed(3)} ms`);
  check("iterations >= 1", report.iterations >= 1, String(report.iterations));
  check("crossOriginIsolated is a boolean", typeof report.crossOriginIsolated === "boolean", String(report.crossOriginIsolated));
  check("constraint_count > 0", envelope.constraint_count > 0, String(envelope.constraint_count));

  console.log(`       workload   ${report.workload_id} (${report.system.proof_system}/${report.system.curve_or_field})`);
  console.log(`       constraints ${envelope.constraint_count}`);
  console.log(`       total      ${report.total_ms.toFixed(3)} ms over ${report.iterations} iteration(s)`);
  console.log(`       clock      ${report.timer_resolution_us.toFixed(4)} µs`);
  console.log(`       memory     ${report.memory.peak_heap_bytes ?? "not measured"}`);
  console.log(`       plan       ${envelope.plan_description}`);
  console.log(`       below      ${envelope.below_target.length === 0 ? "none" : envelope.below_target.join(", ")}`);
  console.log(`       primitives ${summarise(report)}`);
  if (report.phases.length > 0) {
    console.log(`       phases     ${report.phases.map((p) => `${p.name}=${p.ms.toFixed(3)}ms`).join("  ")}`);
  }
  console.log(`       wall       ${wallMs.toFixed(0)} ms (includes setup and iteration planning)`);

  if (args.out) {
    fs.mkdirSync(args.out, { recursive: true });
    const file = path.join(args.out, `zkprof-report-${expected.kind}.json`);
    fs.writeFileSync(file, `${JSON.stringify(report, null, 2)}\n`);
    console.log(`       wrote      ${file}`);
  }
}

async function main() {
  const args = parseArgs(process.argv.slice(2));
  const names = args.only ? [args.only] : Object.keys(PROFILES);
  const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "zkprof-smoke-"));

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
  console.log("Next: validate a report against the schema, then serve the page:");
  console.log("  python web/server.py");
}

try {
  await main();
} catch (error) {
  console.error(`error: ${error && error.message ? error.message : error}`);
  process.exitCode = 1;
}
