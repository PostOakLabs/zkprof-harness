// SPDX-License-Identifier: Apache-2.0
//
// Main-thread controller for the harness page.
//
// Everything measured happens in `worker.js`. This file reads the form, owns
// the worker's lifecycle, and renders what comes back. It computes nothing
// that the Rust side already computed: `below_target` and the plan summary
// arrive in the run envelope precisely so this file never has to re-derive
// them, which is how a page ends up disagreeing with the CLI about whether a
// row is above the noise floor.
//
// The one thing it does compute is presentation: units, percentages, and bar
// widths.

const PROFILES = ["scalar", "simd"];
const PROFILE_LABELS = { scalar: "scalar", simd: "+simd128" };

/** The seven primitives the schema attributes time to, in schema order. */
const ALL_PRIMITIVES = [
  "msm",
  "ntt_fft",
  "hash",
  "field_arith",
  "marshalling",
  "witness_gen",
  "other",
];

const el = (id) => document.getElementById(id);

const state = {
  worker: null,
  profile: "scalar",
  described: null,
  pending: null,
  /** Last completed run per profile, for the side-by-side panel. */
  last: new Map(),
};

// ---------------------------------------------------------------------------
// Formatting
// ---------------------------------------------------------------------------

/** Milliseconds, at a precision that suits the magnitude. */
function formatMs(ms) {
  if (!Number.isFinite(ms)) return "—";
  if (ms === 0) return "0 ms";
  if (ms < 0.001) return `${(ms * 1000).toFixed(3)} µs`;
  if (ms < 1) return `${ms.toFixed(4)} ms`;
  if (ms < 1000) return `${ms.toFixed(3)} ms`;
  return `${(ms / 1000).toFixed(3)} s`;
}

function formatUs(us) {
  if (!Number.isFinite(us)) return "—";
  if (us < 0.001) return `${us.toExponential(2)} µs`;
  if (us < 1) return `${us.toFixed(4)} µs`;
  if (us < 1000) return `${us.toFixed(3)} µs`;
  return `${(us / 1000).toFixed(3)} ms`;
}

function formatBytes(bytes) {
  if (!Number.isFinite(bytes)) return "—";
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 ** 2) return `${(bytes / 1024).toFixed(1)} KiB`;
  if (bytes < 1024 ** 3) return `${(bytes / 1024 ** 2).toFixed(1)} MiB`;
  return `${(bytes / 1024 ** 3).toFixed(2)} GiB`;
}

function formatPercent(share) {
  if (!Number.isFinite(share)) return "—";
  const pct = share * 100;
  if (pct > 0 && pct < 0.01) return "<0.01%";
  return `${pct.toFixed(2)}%`;
}

function formatCount(n) {
  return Number.isFinite(n) ? n.toLocaleString("en-US") : "—";
}

// ---------------------------------------------------------------------------
// DOM helpers
// ---------------------------------------------------------------------------

/**
 * Fill a `<dl class="facts">` from a list of [term, value] pairs.
 *
 * `value` may be a string or a node. A third element marks the value as
 * `ok`/`bad`. Everything goes through `textContent`, never `innerHTML`:
 * `userAgent` reaches this function, and a browser string is not something
 * to hand to an HTML parser.
 */
function renderFacts(dl, pairs) {
  dl.replaceChildren();
  for (const [term, value, tone] of pairs) {
    const dt = document.createElement("dt");
    dt.textContent = term;
    const dd = document.createElement("dd");
    if (value instanceof Node) {
      dd.append(value);
    } else {
      dd.textContent = value;
    }
    if (tone) dd.classList.add(tone);
    dl.append(dt, dd);
  }
}

function emptyRow(table, message, span) {
  const body = table.tBodies[0];
  body.replaceChildren();
  const tr = document.createElement("tr");
  const td = document.createElement("td");
  td.colSpan = span;
  td.textContent = message;
  td.style.color = "var(--ink-faint)";
  tr.append(td);
  body.append(tr);
}

/** A share bar. Width comes from the report; the number sits beside it. */
function bar(share) {
  const outer = document.createElement("span");
  outer.className = "bar";
  outer.setAttribute("aria-hidden", "true");
  const inner = document.createElement("span");
  inner.style.width = `${Math.max(0, Math.min(1, share || 0)) * 100}%`;
  outer.append(inner);
  return outer;
}

// ---------------------------------------------------------------------------
// Page context
// ---------------------------------------------------------------------------

/**
 * What this page can see about the context it is running in.
 *
 * `crossOriginIsolated` is read here and also recorded in the report by the
 * Rust side. Showing both is not duplication: if they disagree, the page is
 * in a context the report does not describe.
 */
function readPageContext() {
  return {
    isolated: self.crossOriginIsolated === true,
    sharedArrayBuffer: typeof SharedArrayBuffer !== "undefined",
    userAgent: navigator.userAgent || "(unavailable)",
    platform: navigator.platform || "(unavailable)",
    hardwareConcurrency: navigator.hardwareConcurrency ?? null,
  };
}

function renderPageContext() {
  const context = readPageContext();
  renderFacts(el("page-context"), [
    ["crossOriginIsolated", String(context.isolated), context.isolated ? "ok" : "bad"],
    ["SharedArrayBuffer", String(context.sharedArrayBuffer)],
    ["Platform", context.platform],
    ["Logical cores", context.hardwareConcurrency === null ? "—" : String(context.hardwareConcurrency)],
    ["User agent", context.userAgent],
  ]);

  const note = el("isolation-note");
  note.classList.remove("isolated", "coarse");
  if (context.isolated) {
    note.classList.add("isolated");
    note.textContent =
      "Cross-origin isolated: the browser is not coarsening performance.now(), " +
      "so timer_resolution_us should land near 5 µs. This is the " +
      "fine-grained context — start the server without --no-isolation.";
  } else {
    note.classList.add("coarse");
    note.textContent =
      "Not cross-origin isolated: the browser coarsens performance.now(), so " +
      "expect timer_resolution_us near 100 µs and treat sub-millisecond rows " +
      "as unresolved. Reports from this context are not comparable with " +
      "isolated ones.";
  }
}

// ---------------------------------------------------------------------------
// Worker lifecycle
// ---------------------------------------------------------------------------

function setStatus(message, tone) {
  const node = el("run-status");
  node.textContent = message;
  node.classList.remove("error", "running");
  if (tone) node.classList.add(tone);
}

function spawnWorker(profile) {
  if (state.worker) {
    state.worker.terminate();
    state.worker = null;
  }
  state.described = null;
  state.profile = profile;

  el("module-status").textContent = `Loading the ${PROFILE_LABELS[profile]} module…`;
  renderFacts(el("module-context"), []);
  setBusy(true, "Loading module…");

  // The profile travels in the query string because the worker needs it
  // before its first message, and a module worker's own URL is the only
  // thing it has at that point.
  const worker = new Worker(`./worker.js?profile=${encodeURIComponent(profile)}`, {
    type: "module",
  });
  worker.addEventListener("message", onWorkerMessage);
  worker.addEventListener("error", (event) => {
    setStatus(`Worker failed to start: ${event.message || "unknown error"}`, "error");
    setBusy(false);
  });
  state.worker = worker;

  send({ type: "describe" });
}

let nextId = 1;

function send(payload) {
  const id = nextId++;
  state.worker.postMessage({ id, ...payload });
  return id;
}

function onWorkerMessage(event) {
  const message = event.data;

  // Stale replies from a previous worker are possible: `terminate()` does not
  // guarantee the in-flight message never arrives.
  if (message.id !== undefined && state.pending && message.id !== state.pending) {
    return;
  }

  if (message.type === "described") {
    state.described = message;
    state.pending = null;
    renderModuleContext(message);
    setBusy(false);
    setStatus("");
    return;
  }

  if (message.type === "result") {
    state.pending = null;
    setBusy(false);
    setStatus("");
    onResult(message);
    return;
  }

  if (message.type === "error") {
    state.pending = null;
    setBusy(false);
    setStatus(message.message, "error");
  }
}

function setBusy(busy, message) {
  const button = el("run-button");
  button.disabled = busy;
  for (const input of el("controls").querySelectorAll("input")) {
    input.disabled = busy;
  }
  if (message) setStatus(message, "running");
}

function renderModuleContext(info) {
  el("module-status").textContent = "Ready.";
  const pairs = [
    ["Profile", PROFILE_LABELS[info.profile]],
    ["target.kind", info.kind],
    ["+simd128", String(info.simd), info.simd ? "ok" : null],
    ["web-threads", String(info.threads), info.threads ? "bad" : null],
  ];
  if (info.threads) {
    pairs.push([
      "Note",
      "The threads profile builds and reports wasm-threads, but the worker " +
        "pool is unimplemented: run_benchmark refuses every call.",
      "bad",
    ]);
  }
  renderFacts(el("module-context"), pairs);
}

// ---------------------------------------------------------------------------
// Results
// ---------------------------------------------------------------------------

function onResult(message) {
  const { envelope, wallMs, profile } = message;
  const report = envelope.report;

  // `size` is the depth or the preimage length, whichever the circuit that
  // produced this run actually used -- recorded so the comparison below can
  // refuse to put a depth-64 row next to a 128-byte row.
  state.last.set(profile, {
    profile,
    envelope,
    wallMs,
    circuit: currentCircuit(),
    size: currentCircuit() === "sha256" ? currentBytes() : currentDepth(),
    seed: currentSeed(),
  });

  el("result-panel").hidden = false;

  const target = report.target || {};
  renderFacts(el("result-summary"), [
    ["Profile", PROFILE_LABELS[profile]],
    ["Workload", report.workload_id],
    ["Proof system", `${report.system.proof_system} / ${report.system.curve_or_field}`],
    ["Constraints", formatCount(envelope.constraint_count)],
    ["Total", formatMs(report.total_ms)],
    ["Per iteration", formatMs(report.total_ms / Math.max(1, report.iterations))],
    ["Iterations", formatCount(report.iterations)],
    ["Clock resolution", formatUs(report.timer_resolution_us)],
    ["Threads", `${report.threads.count}${report.threads.shared_array_buffer ? " (SharedArrayBuffer)" : ""}`],
    [
      "Peak heap",
      report.memory && report.memory.peak_heap_bytes !== undefined
        ? formatBytes(report.memory.peak_heap_bytes)
        : "not measured",
    ],
    ["crossOriginIsolated", String(report.crossOriginIsolated)],
    ["target", `${target.kind} · ${target.os} · ${target.cpu}`],
    ["Wall time", formatMs(wallMs)],
  ]);

  renderPlan(envelope);
  renderPrimitives(report, envelope.below_target);
  renderPhases(report);
  renderRaw(report);
  renderCompare();
}

function renderPlan(envelope) {
  const plan = envelope.plan;
  const callout = el("plan-callout");
  const below = el("below-target-callout");

  callout.hidden = false;
  const verdict = plan.target_met
    ? "Target met: the measured region is at least 1,000× the clock resolution."
    : plan.observed
      ? "Below target: the run was measurable but did not get far enough above the clock. Treat these rows as unresolved."
      : "Unmeasurable: the workload did not register on the clock at all. Widen the workload (raise the depth, or the preimage size) before reading anything here.";
  callout.textContent =
    `${verdict} ${plan.iterations} iteration(s), clock ${formatUs(plan.resolution_us)}` +
    `${plan.calls_spent ? `, ${plan.calls_spent} planning call(s) on top.` : "."}`;
  callout.classList.toggle("callout-warn", !plan.target_met);

  if (envelope.below_target.length === 0) {
    below.hidden = true;
    return;
  }
  below.hidden = false;
  below.textContent =
    `Below the noise floor: ${envelope.below_target.join(", ")}. ` +
    `These rows accumulated less time than ${formatUs(plan.resolution_us)} × 1,000 and must not be compared against anything.`;
}

function renderPrimitives(report, belowTarget) {
  const table = el("primitives-table");
  const body = table.tBodies[0];
  body.replaceChildren();

  const rows = (report.primitives || []).slice();
  if (rows.length === 0) {
    emptyRow(table, "No primitive spans were recorded.", 5);
    return;
  }

  rows.sort((a, b) => b.share - a.share);

  for (const row of rows) {
    const low = belowTarget.includes(row.name);
    const tr = document.createElement("tr");
    if (low) tr.classList.add("below");

    const name = document.createElement("td");
    name.textContent = row.name;

    const ms = document.createElement("td");
    ms.className = "num";
    ms.textContent = formatMs(row.ms);

    const per = document.createElement("td");
    per.className = "num";
    per.textContent = formatMs(row.ms / Math.max(1, report.iterations));

    const share = document.createElement("td");
    share.className = "num";
    share.textContent = formatPercent(row.share);

    const barCell = document.createElement("td");
    barCell.append(bar(row.share));

    tr.append(name, ms, per, share, barCell);
    body.append(tr);
  }

  // Say which of the seven are missing. Absence is instrumentation coverage,
  // not a zero.
  const present = new Set(rows.map((row) => row.name));
  const missing = ALL_PRIMITIVES.filter((name) => !present.has(name));
  if (missing.length > 0) {
    const tr = document.createElement("tr");
    const td = document.createElement("td");
    td.colSpan = 5;
    td.style.color = "var(--ink-faint)";
    td.style.fontSize = "0.8rem";
    td.textContent = `Not instrumented by this backend: ${missing.join(", ")}.`;
    tr.append(td);
    body.append(tr);
  }
}

function renderPhases(report) {
  const table = el("phases-table");
  const body = table.tBodies[0];
  body.replaceChildren();

  const rows = (report.phases || []).slice();
  if (rows.length === 0) {
    emptyRow(
      table,
      "No phase spans: this backend times primitives only, so there is nothing here yet.",
      4,
    );
    return;
  }

  rows.sort((a, b) => b.share - a.share);
  for (const row of rows) {
    const tr = document.createElement("tr");
    const name = document.createElement("td");
    name.textContent = row.name;
    const ms = document.createElement("td");
    ms.className = "num";
    ms.textContent = formatMs(row.ms);
    const share = document.createElement("td");
    share.className = "num";
    share.textContent = formatPercent(row.share);
    const barCell = document.createElement("td");
    barCell.append(bar(row.share));
    tr.append(name, ms, share, barCell);
    body.append(tr);
  }
}

function renderRaw(report) {
  el("raw-json").textContent = JSON.stringify(report, null, 2);
}

/**
 * Side-by-side of the two profiles.
 *
 * Only shown when both runs share a circuit, a size and a seed. Comparing a
 * depth-64 Merkle run with a 128-byte SHA-256 run, or two runs at different
 * depths, would put two numbers next to each other that do not measure the
 * same thing.
 */
function renderCompare() {
  const panel = el("compare-panel");
  const note = el("compare-note");
  const body = el("compare-table").tBodies[0];

  const runs = PROFILES.map((name) => state.last.get(name)).filter(Boolean);
  if (runs.length < 2) {
    panel.hidden = true;
    return;
  }

  const [first] = runs;
  const comparable = runs.every(
    (run) =>
      run.circuit === first.circuit && run.size === first.size && run.seed === first.seed,
  );

  panel.hidden = false;
  body.replaceChildren();

  /** `depth 64` or `128 bytes`, whichever this circuit's size knob is. */
  const describeRun = (run) =>
    `${run.circuit} ${
      run.circuit === "sha256" ? `${run.size} bytes` : `depth ${run.size}`
    }, seed ${run.seed}`;

  if (!comparable) {
    note.textContent =
      "The two runs used different circuits, sizes or seeds, so their totals " +
      "do not measure the same workload. Re-run at matching settings to compare.";
    body.replaceChildren();
    for (const run of runs) {
      const tr = document.createElement("tr");
      const name = document.createElement("td");
      name.textContent = `${PROFILE_LABELS[run.profile]} (${describeRun(run)})`;
      tr.append(name);
      body.append(tr);
    }
    return;
  }

  note.textContent =
    `Both runs on ${describeRun(first)}. "Relative" is ` +
    `total time against the scalar build; it is one browser, one session, ` +
    `one sample — not a benchmark.`;

  const baseline = state.last.get("scalar");
  for (const name of PROFILES) {
    const run = state.last.get(name);
    if (!run) continue;
    const total = run.envelope.report.total_ms;
    const tr = document.createElement("tr");

    const label = document.createElement("td");
    label.textContent = PROFILE_LABELS[name];

    const totalCell = document.createElement("td");
    totalCell.className = "num";
    totalCell.textContent = formatMs(total);

    const iterCell = document.createElement("td");
    iterCell.className = "num";
    iterCell.textContent = formatCount(run.envelope.report.iterations);

    const clockCell = document.createElement("td");
    clockCell.className = "num";
    clockCell.textContent = formatUs(run.envelope.report.timer_resolution_us);

    const relCell = document.createElement("td");
    relCell.className = "num";
    relCell.textContent =
      baseline && name === "simd"
        ? `${(total / baseline.envelope.report.total_ms).toFixed(3)}×`
        : "1.000×";

    tr.append(label, totalCell, iterCell, clockCell, relCell);
    body.append(tr);
  }
}

// ---------------------------------------------------------------------------
// Controls
// ---------------------------------------------------------------------------

function currentCircuit() {
  const value = el("circuit").value;
  return value === "sha256" ? "sha256" : "merkle-poseidon";
}

/**
 * The preimage size, or null for a circuit it does not apply to.
 *
 * Null means "let the crate choose", which is what keeps the two circuits
 * from having to agree on what a size means. The Rust side refuses a size
 * that is not one csp-benchmarks measures, so this list is not the only gate.
 */
function currentBytes() {
  if (currentCircuit() !== "sha256") return null;
  const value = Number(el("bytes").value);
  return Number.isInteger(value) && value >= 1 ? value : 128;
}

/**
 * Show the size knob that belongs to the selected circuit.
 *
 * Hiding rather than disabling: a disabled control still looks like a setting
 * the run will use, and "which circuit am I measuring" is exactly the thing
 * this page must not make ambiguous.
 */
function onCircuitChange() {
  const sha256 = currentCircuit() === "sha256";
  el("depth-control").hidden = sha256;
  el("bytes-control").hidden = !sha256;
  el("depth").disabled = sha256;
  el("bytes").disabled = !sha256;
}

function currentDepth() {
  const value = Number(el("depth").value);
  return Number.isInteger(value) && value >= 1 && value <= 128 ? value : 64;
}

function currentSeed() {
  const value = Number(el("seed").value);
  // u64. Above 2^53 the Number type stops being exact, so the page refuses
  // rather than silently rounding the seed the report claims to have used.
  if (!Number.isInteger(value) || value < 0 || !Number.isSafeInteger(value)) return 24237;
  return value;
}

function onSubmit(event) {
  event.preventDefault();
  if (!state.worker) return;

  setBusy(
    true,
    "Running. The workload blocks the worker — the page stays responsive, " +
      "but nothing is reported until it finishes.",
  );
  state.pending = send({
    type: "run",
    circuit: currentCircuit(),
    depth: currentDepth(),
    bytes: currentBytes(),
    seed: currentSeed(),
  });
}

function onProfileChange(event) {
  spawnWorker(event.target.value);
}

async function onDownload() {
  const run = state.last.get(state.profile);
  if (!run) return;
  const report = run.envelope.report;
  const kind = report.target.kind;
  const stamp = new Date().toISOString().replace(/[:.]/g, "-");
  const json = JSON.stringify(report, null, 2);

  const blob = new Blob([json], { type: "application/json" });
  const url = URL.createObjectURL(blob);
  const anchor = document.createElement("a");
  anchor.href = url;
  anchor.download = `zkprof-report-${kind}-${stamp}.json`;
  document.body.append(anchor);
  anchor.click();
  anchor.remove();
  URL.revokeObjectURL(url);
}

async function onCopy() {
  const run = state.last.get(state.profile);
  if (!run) return;
  const json = JSON.stringify(run.envelope.report, null, 2);
  try {
    await navigator.clipboard.writeText(json);
    el("copy-button").textContent = "Copied";
    setTimeout(() => {
      el("copy-button").textContent = "Copy JSON";
    }, 1500);
  } catch {
    setStatus("Could not reach the clipboard; use Download instead.", "error");
  }
}

function main() {
  renderPageContext();
  el("controls").addEventListener("submit", onSubmit);
  el("circuit").addEventListener("change", onCircuitChange);
  onCircuitChange();
  for (const input of document.querySelectorAll('input[name="profile"]')) {
    input.addEventListener("change", onProfileChange);
  }
  el("download-button").addEventListener("click", onDownload);
  el("copy-button").addEventListener("click", onCopy);
  spawnWorker("scalar");
}

main();
