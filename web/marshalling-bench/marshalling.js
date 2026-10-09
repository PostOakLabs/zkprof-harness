// SPDX-License-Identifier: Apache-2.0
//
// Controller for the marshalling bench page.
//
// The loop lives here, and that is the whole design. A Rust loop never
// crosses the JS<->WASM boundary, so a Rust loop cannot measure it; this file
// owns the loop, times it, and hands the elapsed time back to the module,
// which attributes everything it cannot account for to `marshalling`.
//
// This file computes nothing that the Rust side already computed. The
// per-call, per-hash and per-KiB figures arrive in the run envelope, already
// divided out of the report -- that is the point of `host::Derived`. Redoing
// the division here is how a page and a CLI end up printing different answers
// from the same report.
//
// Everything it does compute is presentation: units, and the bar under the
// marshalling share.

const PROFILES = ["scalar", "simd"];
const PROFILE_LABELS = { scalar: "scalar", simd: "+simd128" };

/**
 * What each profile must report as `target.kind`.
 *
 * The compiled code decides this from `cfg!`, not from anything here, so a
 * mismatch means the page loaded a different module than it asked for --
 * which a stale build makes entirely possible.
 */
const EXPECTED_KIND = { scalar: "wasm-scalar", simd: "wasm-simd" };

/** Enough crossings to size a loop; too few to be a measurement. */
const PROBE_CALLS = 256;

/**
 * Ceiling on the sized loop.
 *
 * A coarsened clock can ask for a very large iteration count. Browsers will
 * not let this page block for minutes, and a run the user cannot wait out is
 * a run whose result never arrives, so the count is capped and the cap is
 * reported rather than hidden.
 */
const MAX_MEASURED_CALLS = 50_000;

const el = (id) => document.getElementById(id);

const state = {
  modules: new Map(),
  profile: "scalar",
  rows: [],
  last: null,
  running: false,
};

// ---------------------------------------------------------------------------
// Formatting
// ---------------------------------------------------------------------------

function formatUs(us) {
  if (us === null || us === undefined || !Number.isFinite(us)) return "—";
  if (us < 0.001) return us.toExponential(2);
  if (us < 1) return us.toFixed(4);
  if (us < 1000) return us.toFixed(3);
  return (us / 1000).toFixed(3);
}

function formatBytes(bytes) {
  if (!Number.isFinite(bytes)) return "—";
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 ** 2) return `${(bytes / 1024).toFixed(1)} KiB`;
  return `${(bytes / 1024 ** 2).toFixed(2)} MiB`;
}

function formatCount(n) {
  return Number.isFinite(n) ? n.toLocaleString("en-US") : "—";
}

function formatPercent(share) {
  if (!Number.isFinite(share)) return "—";
  const pct = share * 100;
  if (pct > 0 && pct < 0.01) return "<0.01%";
  return `${pct.toFixed(2)}%`;
}

/** `textContent` only, never `innerHTML`: userAgent reaches the page. */
function renderFacts(dl, pairs) {
  dl.replaceChildren();
  for (const [term, value, tone] of pairs) {
    const dt = document.createElement("dt");
    dt.textContent = term;
    const dd = document.createElement("dd");
    dd.textContent = value;
    if (tone) dd.classList.add(tone);
    dl.append(dt, dd);
  }
}

function setStatus(message, tone) {
  const node = el("run-status");
  node.textContent = message;
  node.classList.remove("error", "running");
  if (tone) node.classList.add(tone);
}

// ---------------------------------------------------------------------------
// Deployment context
// ---------------------------------------------------------------------------

/**
 * `crossOriginIsolated` is read here and recorded in the report by the Rust
 * side. Showing both is not duplication: if they disagree, the bench is in a
 * context the report does not describe.
 *
 * This matters more here than anywhere else in the repo. A boundary crossing
 * is the smallest thing measured, and an un-isolated page runs on a clock
 * coarse enough that sub-millisecond rows are unresolved.
 */
function readPageContext() {
  return {
    isolated: self.crossOriginIsolated === true,
    userAgent: navigator.userAgent || "(unavailable)",
    platform: navigator.platform || "(unavailable)",
    hardwareConcurrency: navigator.hardwareConcurrency ?? null,
  };
}

function renderPageContext() {
  const context = readPageContext();
  renderFacts(el("page-context"), [
    ["crossOriginIsolated", String(context.isolated), context.isolated ? "ok" : "bad"],
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
      "so timer_resolution_us should land near 5 µs. Start the server without " +
      "--no-isolation for this context.";
  } else {
    note.classList.add("coarse");
    note.textContent =
      "Not cross-origin isolated: the browser coarsens performance.now(), so " +
      "expect timer_resolution_us near 100 µs. A boundary crossing is smaller " +
      "than that, so rows below a millisecond are unresolved here and reports " +
      "from this context are not comparable with isolated ones.";
  }
}

// ---------------------------------------------------------------------------
// Module loading
// ---------------------------------------------------------------------------

/**
 * Instantiate one profile's module, once.
 *
 * The glue resolves the wasm relative to the document, so a bare
 * `import()` works and no explicit byte fetch is needed -- unlike the Node
 * smoke script, which has to stage the glue into a temp directory.
 */
async function moduleFor(profile) {
  if (state.modules.has(profile)) return state.modules.get(profile);

  const url = new URL(`./pkg-${profile}/zkprof_marshal.js`, import.meta.url);
  const glue = await import(url.href);
  await glue.default();
  state.modules.set(profile, glue);
  return glue;
}

async function showModule(profile) {
  const status = el("module-status");
  status.textContent = `Loading the ${PROFILE_LABELS[profile]} module…`;
  try {
    const glue = await moduleFor(profile);
    const target = JSON.parse(glue.marshal_target_json());
    const suite = JSON.parse(glue.marshal_suite());
    const resolution = glue.marshal_timer_resolution_us();

    status.textContent = "Ready.";
    // Only fields the schema's `target` block actually carries. `gpu` is
    // optional and absent for every wasm target this module can be.
    renderFacts(el("module-context"), [
      ["target.kind", target.kind, target.kind === EXPECTED_KIND[profile] ? "ok" : "bad"],
      ["+simd128", String(glue.marshal_simd_enabled())],
      ["Browser", target.browser ?? "—"],
      ["OS", target.os],
      ["CPU", target.cpu],
      ["Clock resolution", `${formatUs(resolution)} µs`],
      ["Workloads", String(suite.length)],
    ]);
    return glue;
  } catch (error) {
    status.textContent = `Could not load the ${PROFILE_LABELS[profile]} module.`;
    setStatus(
      `${error && error.message ? error.message : error}\n` +
        "Build it with: python tools/build_wasm.py --module marshal",
      "error",
    );
    return null;
  }
}

// ---------------------------------------------------------------------------
// The measured loop
// ---------------------------------------------------------------------------

/**
 * Time `calls` crossings over one payload.
 *
 * The returned bytes are touched so the outbound half of the crossing stays
 * in the timed region. `elapsed` is what the report is built from; the host
 * is the only thing that can see the whole crossing, which is why it is the
 * host that times it.
 */
function timeLoop(glue, payload, calls) {
  const started = performance.now();
  let sink = 0;
  for (let i = 0; i < calls; i += 1) {
    sink += glue.marshal_call(payload)[0];
  }
  return { elapsed: performance.now() - started, sink };
}

function payloadFor(length) {
  const payload = new Uint8Array(length);
  for (let i = 0; i < length; i += 1) payload[i] = (i * 31 + 7) & 0xff;
  return payload;
}

async function runSuite(glue) {
  const suite = JSON.parse(glue.marshal_suite());
  const rows = [];

  for (let index = 0; index < suite.length; index += 1) {
    const spec = suite[index];
    setStatus(`Running ${index + 1} of ${suite.length}: ${spec.workload_id}`, "running");
    // Yield so the status actually paints before the loop blocks the thread.
    await new Promise((resolve) => setTimeout(resolve, 0));

    glue.marshal_begin(spec.kind, spec.param);
    const payload = payloadFor(glue.marshal_payload_len());

    const probe = timeLoop(glue, payload, PROBE_CALLS);
    const plan = JSON.parse(glue.marshal_size(PROBE_CALLS, probe.elapsed));
    const calls = Math.min(plan.iterations, MAX_MEASURED_CALLS);

    const measured = timeLoop(glue, payload, calls);
    const envelope = JSON.parse(glue.marshal_report(calls, measured.elapsed));

    rows.push({
      spec,
      plan,
      calls,
      capped: plan.iterations > MAX_MEASURED_CALLS,
      report: envelope.report,
      derived: envelope.derived,
    });
  }

  return rows;
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

function marshallingShare(report) {
  const row = report.primitives.find((primitive) => primitive.name === "marshalling");
  return row ? row.share : 0;
}

function renderResults(rows, profile) {
  const body = el("results-table").tBodies[0];
  body.replaceChildren();

  for (const { spec, report, derived, calls, capped } of rows) {
    const tr = document.createElement("tr");

    const name = document.createElement("th");
    name.scope = "row";
    name.textContent = spec.label;
    const id = document.createElement("div");
    id.className = "workload-id";
    id.textContent = spec.workload_id;
    name.append(id);
    tr.append(name);

    const cells = [
      capped ? `${formatCount(calls)} *` : formatCount(calls),
      formatBytes(derived.bytes_per_call),
      formatUs(derived.us_per_call),
      derived.us_per_hash === null ? "—" : formatUs(derived.us_per_hash),
      formatUs(derived.us_per_kib),
      formatPercent(marshallingShare(report)),
    ];
    for (const text of cells) {
      const td = document.createElement("td");
      td.className = "num";
      td.textContent = text;
      tr.append(td);
    }

    const actions = document.createElement("td");
    const button = document.createElement("button");
    button.type = "button";
    button.className = "link";
    button.textContent = "JSON";
    button.addEventListener("click", () => showRaw(report));
    actions.append(button);
    tr.append(actions);

    // The share gets a bar: this is the row the bench exists to produce, and
    // "how much of this call was the crossing" reads better as a length.
    const share = tr.children[6];
    const bar = document.createElement("div");
    bar.className = "share-bar";
    const fill = document.createElement("div");
    fill.style.width = `${Math.max(0, Math.min(1, marshallingShare(report))) * 100}%`;
    bar.append(fill);
    share.append(bar);

    body.append(tr);
  }

  const capped = rows.filter((row) => row.capped);
  el("result-note").textContent =
    `${rows.length} workloads on the ${PROFILE_LABELS[profile]} profile. ` +
    `Measured in this browser just now; nothing here is a published benchmark.` +
    (capped.length > 0
      ? ` ${capped.length} workload(s) hit the ${formatCount(MAX_MEASURED_CALLS)}-call ceiling (*), ` +
        "because the clock this page was given is coarse enough to ask for more."
      : "");

  const callout = el("resolution-callout");
  callout.hidden = false;
  callout.replaceChildren();
  for (const { plan, spec, capped: wasCapped } of rows) {
    if (!wasCapped && plan.target_met) continue;
    const line = document.createElement("p");
    line.textContent = `${spec.workload_id}: ${plan.description}`;
    callout.append(line);
  }
  if (callout.childElementCount === 0) {
    callout.hidden = true;
  }
}

function showRaw(report) {
  state.last = report;
  el("raw-json").textContent = JSON.stringify(report, null, 2);
}

// ---------------------------------------------------------------------------
// Actions
// ---------------------------------------------------------------------------

function onDownloadAll() {
  if (state.rows.length === 0) return;
  // One file per workload, because the batch size lives in `workload_id` and
  // not in a field of its own: there is no way to put two batch sizes in one
  // report, and merging them would mean inventing a schema extension.
  for (const { report } of state.rows) {
    const json = JSON.stringify(report, null, 2);
    const blob = new Blob([json], { type: "application/json" });
    const url = URL.createObjectURL(blob);
    const anchor = document.createElement("a");
    anchor.href = url;
    anchor.download = `${report.target.kind}-${report.workload_id}.json`;
    document.body.append(anchor);
    anchor.click();
    anchor.remove();
    URL.revokeObjectURL(url);
  }
}

async function onCopy() {
  const report = state.last ?? (state.rows.at(-1)?.report ?? null);
  if (!report) return;
  try {
    await navigator.clipboard.writeText(JSON.stringify(report, null, 2));
    el("copy-button").textContent = "Copied";
    setTimeout(() => {
      el("copy-button").textContent = "Copy the last report";
    }, 1500);
  } catch {
    setStatus("Could not reach the clipboard; use Download instead.", "error");
  }
}

async function onSubmit(event) {
  event.preventDefault();
  if (state.running) return;

  const profile = new FormData(event.target).get("profile") ?? "scalar";
  state.profile = profile;
  state.running = true;
  el("run-button").disabled = true;

  try {
    const glue = await showModule(profile);
    if (!glue) return;

    setStatus("Running the suite…", "running");
    state.rows = await runSuite(glue);
    state.last = state.rows.at(-1)?.report ?? null;

    renderResults(state.rows, profile);
    el("result-panel").hidden = false;
    if (state.last) showRaw(state.last);
    setStatus(`Done: ${state.rows.length} workloads measured.`);
  } catch (error) {
    setStatus(`${error && error.message ? error.message : error}`, "error");
    glueAbort();
  } finally {
    state.running = false;
    el("run-button").disabled = false;
  }
}

/** A half-finished bench must not survive into the next run. */
function glueAbort() {
  for (const glue of state.modules.values()) {
    try {
      glue.marshal_abort();
    } catch {
      // A module that will not abort is the least of the problems here.
    }
  }
}

// ---------------------------------------------------------------------------
// Wiring
// ---------------------------------------------------------------------------

function selectedProfile() {
  const checked = document.querySelector('input[name="profile"]:checked');
  return checked ? checked.value : "scalar";
}

async function onProfileChange() {
  const profile = selectedProfile();
  state.profile = profile;
  await showModule(profile);
}

async function main() {
  renderPageContext();
  el("controls").addEventListener("submit", onSubmit);
  el("download-button").addEventListener("click", onDownloadAll);
  el("copy-button").addEventListener("click", onCopy);
  for (const input of document.querySelectorAll('input[name="profile"]')) {
    input.addEventListener("change", onProfileChange);
  }
  await onProfileChange();
  setStatus("");
}

if (!PROFILES.includes(selectedProfile())) {
  setStatus("Unknown profile in the form.", "error");
} else {
  await main();
}
