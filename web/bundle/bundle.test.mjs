// SPDX-License-Identifier: Apache-2.0
//
// RV5 test — the combined receipt-bundle verifier over the five committed
// fixtures. Node 18+, no dependencies (this file is
// committed, runnable by anyone, and run by the landing row; the CI gate is
// Rust/Python, and adding a Node job for this file is a separate decision).
//
//   node web/bundle/bundle.test.mjs
//
// What this test proves, beyond "the green cases are green":
//   * each RED control fails in EXACTLY its own half, with the other half
//     still passing — the halves are independently decidable and the verifier
//     fails honestly (a new gate proves RED before GREEN, SO #34c discipline);
//   * the machine-readable verdict NEVER collapses the two claims: `proof` and
//     `logged` are siblings with their own verdict tokens, and no field in the
//     verdict object asserts both at once;
//   * the exit code requires both halves but the per-half verdicts remain
//     separately legible.

import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { verifyBundle } from './verify-bundle.mjs';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const fixturesDir = path.resolve(__dirname, '..', '..', 'crates', 'zkprof-verify', 'fixtures', 'bundles');

let failures = 0;
function assert(cond, label) {
  console.log(`${cond ? 'PASS' : 'FAIL'}  ${label}`);
  if (!cond) failures++;
}

function assertNoCollapsedClaim(verdict, label) {
  // the non-collapse invariant, stated structurally: exactly two top-level
  // half-verdicts under proof/logged, a combined object that only QUOTES the
  // two tokens, and no boolean anywhere asserting both halves at once.
  const okShape =
    verdict.proof && typeof verdict.proof.verdict === 'string' &&
    verdict.logged && typeof verdict.logged.verdict === 'string' &&
    verdict.combined && typeof verdict.combined.proof === 'string' &&
    typeof verdict.combined.logged === 'string' &&
    typeof verdict.combined.note === 'string';
  const noSingleClaim =
    verdict.valid === undefined && verdict.ok === undefined && verdict.all === undefined &&
    verdict.combined.valid === undefined && verdict.combined.ok === undefined &&
    typeof verdict.combined.proof === 'string' && typeof verdict.combined.logged === 'string';
  assert(okShape && noSingleClaim, `${label}: verdict shape keeps proof/logged as separate claims (no collapsed field)`);
}

async function main() {
  const names = fs.readdirSync(fixturesDir).filter((f) => f.endsWith('.bundle.json')).sort();
  assert(names.length === 5, `exactly five bundle fixtures committed (found ${names.length}: ${names.join(', ')})`);

  // ---- GREEN: three real bundles, both halves pass ----
  for (const name of ['real-art-01.bundle.json', 'real-art-04.bundle.json', 'real-art-106.bundle.json']) {
    const bundle = JSON.parse(fs.readFileSync(path.join(fixturesDir, name), 'utf8'));
    const v = await verifyBundle(bundle);
    assert(v.proof.verdict === 'PROOF-VALID', `${name}: PROOF half valid`);
    assert(v.logged.verdict === 'LOGGED', `${name}: LOGGED half valid`);
    assert(v.combined.proof === 'VALID' && v.combined.logged === 'VALID', `${name}: combined states both claims`);
    assertNoCollapsedClaim(v, name);
  }

  // art-04 is the §18.7 discriminating-journal receipt: its journal has
  // unsorted nested keys, so the JCS recompute is load-bearing here (an
  // insertion-order serializer would produce a different digest and the
  // pairing check would reject the receipt).
  {
    const bundle = JSON.parse(fs.readFileSync(path.join(fixturesDir, 'real-art-04.bundle.json'), 'utf8'));
    const v = await verifyBundle(bundle);
    const jcs = v.proof.checks.find((c) => c.name === 'claim digest derived');
    assert(!!jcs && jcs.passed === true, 'real-art-04: §18.7 JCS journal recompute passes on the discriminating journal');
    const advisory = v.proof.checks.find((c) => c.name.startsWith('journal_digest annotation'));
    assert(!!advisory && advisory.passed === true, 'real-art-04: advisory journal_digest annotation matches the recompute');
  }

  // ---- RED: bad seal — proof half fails, logged half STILL passes ----
  {
    const bundle = JSON.parse(fs.readFileSync(path.join(fixturesDir, 'red-seal-art-04.bundle.json'), 'utf8'));
    const v = await verifyBundle(bundle);
    assert(v.proof.verdict === 'PROOF-INVALID', 'red-seal: PROOF half invalid');
    const failing = v.proof.checks.filter((c) => c.passed === false).map((c) => c.name);
    assert(failing.includes('Groth16 pairing check'), `red-seal: failure lands in the pairing check (${failing.join('; ')})`);
    assert(v.logged.verdict === 'LOGGED', 'red-seal: LOGGED half STILL valid (halves independently decidable)');
    assert(v.combined.proof === 'INVALID' && v.combined.logged === 'VALID', 'red-seal: combined states proof=INVALID, logged=VALID');
    assertNoCollapsedClaim(v, 'red-seal');
  }

  // ---- RED: bad inclusion proof — logged half fails, proof half STILL passes ----
  {
    const bundle = JSON.parse(fs.readFileSync(path.join(fixturesDir, 'red-inclusion-art-04.bundle.json'), 'utf8'));
    const v = await verifyBundle(bundle);
    assert(v.logged.verdict === 'NOT-LOGGED', 'red-inclusion: LOGGED half invalid');
    const failing = v.logged.checks.filter((c) => c.passed === false).map((c) => c.name);
    assert(failing.includes('RFC 6962 inclusion proof -> tree_head root'), `red-inclusion: failure lands in the inclusion walk (${failing.join('; ')})`);
    assert(v.proof.verdict === 'PROOF-VALID', 'red-inclusion: PROOF half STILL valid (halves independently decidable)');
    assert(v.combined.proof === 'VALID' && v.combined.logged === 'INVALID', 'red-inclusion: combined states proof=VALID, logged=INVALID');
    assertNoCollapsedClaim(v, 'red-inclusion');
  }

  console.log(failures === 0 ? '\nbundle.test: ALL PASS' : `\nbundle.test: ${failures} FAILURE(S)`);
  process.exit(failures === 0 ? 0 : 1);
}

main().catch((e) => { console.error(e); process.exit(2); });
