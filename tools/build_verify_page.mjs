// SPDX-License-Identifier: Apache-2.0
//
// Build the self-contained receipt-verifier page (web/verify/index.html) from:
//   - web/verify/index.template.html  (the page skeleton + wiring)
//   - web/verify/pkg/zkprof_verify.js (the wasm-bindgen glue, inlined)
//   - web/verify/pkg/zkprof_verify_bg.wasm (base64-embedded for zero network)
//   - crates/zkprof-verify/fixtures/compute-proof/art-01.receipt.json (sample)
//
// Zero network after load: the wasm is decoded from base64 in memory and
// instantiated via WebAssembly.Module (initSync), never fetched. Run after
// building the wasm:
//   node tools/build_verify_page.mjs
// Prints the size of the glue, the base64 wasm, and the resulting page.

import { readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "..");

const template = readFileSync(join(root, "web/verify/index.template.html"), "utf8");
const glue = readFileSync(join(root, "web/verify/pkg/zkprof_verify.js"), "utf8");
const wasm = readFileSync(join(root, "web/verify/pkg/zkprof_verify_bg.wasm"));
const sample = readFileSync(join(root, "crates/zkprof-verify/fixtures/compute-proof/art-01.receipt.json"), "utf8");

const wasmB64 = wasm.toString("base64");
const sampleJson = JSON.stringify(JSON.parse(sample)); // canonical single-line

let page = template
  .replace("__GLUE__", () => glue)
  .replace("__SAMPLE_RECEIPT_UPDATABLE_JSON__", () => sampleJson)
  .replace('"__WASM_B64__"', () => '"' + wasmB64 + '"');

writeFileSync(join(root, "web/verify/index.html"), page, "utf8");

console.log("glue js bytes:", Buffer.byteLength(glue));
console.log("wasm bytes:", wasm.length, "base64 length:", wasmB64.length);
console.log("page bytes:", Buffer.byteLength(page));
