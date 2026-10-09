// SPDX-License-Identifier: Apache-2.0
//
// The measured region lives here, in a module worker, and nowhere else.
//
// `run_benchmark` is synchronous and blocks whatever thread calls it. On the
// main thread that is a frozen tab and, in some browsers, a "this page is
// unresponsive" kill prompt -- during a run that can last a minute. Blocking a
// worker costs nothing but the worker, and the main thread stays free to
// repaint.
//
// The module is imported dynamically because the profile is not known until
// the worker is constructed. The specifier is relative, so it resolves against
// this file's own URL, which is also what lets wasm-bindgen's glue find its
// `.wasm` sibling: that glue resolves `zkprof_wasm_bg.wasm` against
// `import.meta.url`, not against the page.

const PROFILES = new Set(["scalar", "simd"]);

const requested = new URLSearchParams(self.location.search).get("profile");
const profile = PROFILES.has(requested) ? requested : "scalar";

let loaded = null;

/** Import the profile's glue and instantiate the module. Idempotent. */
async function loadModule() {
  if (loaded) return loaded;
  const glue = await import(`./pkg-${profile}/zkprof_wasm.js`);
  // The default export is wasm-bindgen's async `__wbg_init`. Called with no
  // argument it fetches the `.wasm` next to the glue, so the server only has
  // to serve the directory -- no paths are hard-coded here.
  await glue.default();
  loaded = glue;
  return glue;
}

/**
 * Everything the module can say about itself without running anything.
 *
 * `target_json` is parsed here rather than forwarded as a string so the page
 * never has to know whether it is looking at JSON or an object.
 */
async function describe() {
  const glue = await loadModule();
  return {
    profile,
    kind: glue.target_kind(),
    simd: glue.simd_enabled(),
    threads: glue.threads_enabled(),
    target: JSON.parse(glue.target_json()),
  };
}

/** Turn whatever crossed the wasm boundary into a message worth showing. */
function describeError(error) {
  if (error instanceof Error) return error.message;
  return String(error);
}

self.addEventListener("message", async (event) => {
  const { id, type } = event.data;

  const reply = (payload) => self.postMessage({ id, ...payload });

  try {
    if (type === "describe") {
      reply({ type: "described", ...(await describe()) });
      return;
    }

    if (type === "run") {
      const glue = await loadModule();

      // `circuit` is the only argument that is a string; `seed` is a `u64`,
      // which wasm-bindgen maps to BigInt and which throws inside the glue if
      // it arrives as a Number. `depth` and `bytes` are plain u32s. All four
      // accept null, which becomes Rust's None and selects the crate's
      // default -- so passing a size that only makes sense for the *other*
      // circuit is harmless rather than a mismatch.
      const { circuit, depth, bytes, seed } = event.data;
      const startedAt = performance.now();
      const json = glue.run_benchmark(
        circuit ?? null,
        depth === null || depth === undefined ? null : Number(depth),
        bytes === null || bytes === undefined ? null : Number(bytes),
        seed === null || seed === undefined ? null : BigInt(seed),
      );

      // Wall time around the call, for the page's "how long did that take"
      // line only. It is deliberately not part of the report: it includes the
      // iteration planning that the report's own total_ms excludes.
      const wallMs = performance.now() - startedAt;

      reply({ type: "result", envelope: JSON.parse(json), wallMs, profile });
      return;
    }

    reply({ type: "error", message: `unknown message type: ${type}` });
  } catch (error) {
    reply({ type: "error", message: describeError(error) });
  }
});
