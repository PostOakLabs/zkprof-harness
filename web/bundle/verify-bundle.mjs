// SPDX-License-Identifier: Apache-2.0
//
// RV5 — the combined receipt-bundle verifier: consumes a RECEIPT-BUNDLE v0.1
// JSON file (PostOakLabs/ainumbers chaingraph/standard/RECEIPT-BUNDLE.md) and
// verifies BOTH halves offline, in one command:
//
//   PROOF half  — the snarkjs independent twin (web/twin/verify.mjs, unchanged):
//                 groth16-bn254 seal over ReceiptClaim::ok(imageId, journalBytes)
//                 with journalBytes = utf8(JCS(journal)) per SPEC.md §18.7.
//   LOGGED half — an INDEPENDENT reimplementation (this file) of the Sigsum /
//                 C2SP verification chain, from the primary sources cited in
//                 the comment block below. It shares nothing at runtime with
//                 the estate's register-sigsum.mjs (that is the point of the
//                 twin discipline: implementation independence).
//
// THE NON-COLLAPSE RULE (bundle SPEC §3.3): the output states the two
// half-verdicts explicitly and separately. "proof valid" and "logged" are
// different statements about different subjects; neither implies the other,
// and this tool never emits a single claim covering both. The machine-readable
// verdict carries `proof` and `logged` as siblings; the human output prints
// two labelled verdict lines. Exit code 0 requires BOTH halves to pass, but
// the two verdicts stay separately legible in every output mode.
//
// Offline invariant (SO #0 / CHAINPOINT GUARD discipline): zero network
// operations. Every key is PINNED in this file's trust-policy constants
// (copied from the published policy files, never fetched at verify time).
//
// Sigsum/C2SP semantics reimplemented here from the primary sources (same
// citations the estate's own script records, read from the specs/sources —
// this code was written against the wire format, not translated from the
// estate's implementation):
//   C2SP tlog-checkpoint (v1.0.0)  — checkpoint = "<origin>\n<size>\n<root-b64>\n"
//                                     [+ extension lines] as a signed note body
//   C2SP signed-note (v1.0.0)      — "— <name> <base64(keyHint(4) || sig)>" lines
//   C2SP tlog-cosignature (v1.0.1) — message "cosignature/v1\ntime <ts>\n<checkpoint>"
//   RFC 6962 §2.1.3.2              — 0x00/0x01-prefixed leaf/interior hashing,
//                                     iterative inclusion walk
//   sigsum leaf                    — Leaf{Checksum,Signature,KeyHash} serializes
//                                     as checksum‖signature‖keyHash (128 bytes);
//                                     the leaf signature domain-separates with
//                                     "sigsum.org/v1/tree-leaf"; the checkpoint
//                                     origin is "sigsum.org/v1/tree/" + sha256(log key)
//
// Usage:
//   node web/bundle/verify-bundle.mjs <bundle.json> [more.json ...]
//   node web/bundle/verify-bundle.mjs --json <bundle.json>    # machine verdicts
//
// Exit 0 iff every bundle's PROOF and LOGGED halves both pass.

import fs from 'node:fs';
import path from 'node:path';
import { webcrypto } from 'node:crypto';
import { verify as twinVerify, deriveDigests } from '../twin/verify.mjs';

const subtle = webcrypto.subtle;

// ---------------------------------------------------------------------------
// Pinned trust anchors — the "12-pin set" identifier's substance (bundle SPEC
// §2 trust_policy). Transcribed from the PUBLISHED policy files in
// PostOakLabs/ainumbers, pinned by content hash here so a future edit of the
// policy files cannot silently move under an old bundle:
//
//   chaingraph/policies/sigsum-generic-2025-1.tlog-policy
//     sha256 666d9d0b9ab2e4019769c42eaccd7d6d502a9abac45979bb9b08b7213e4f53e3
//     (2 logs, 3 witnesses, quorum 2 of 3)
//   chaingraph/policies/ainumbers-registry-lineage.tlog-policy
//   chaingraph/policies/ainumbers-registry-errata.tlog-policy
//     (operator keys of the two self-run registry logs the v0.1 anchors commit to)
//
// The bundle carries the policy NAME and sha256 for humans; this verifier
// decides from its own constants (bundle SPEC §3.2 step 1: never adopt a trust
// policy out of the artifact under test).
// ---------------------------------------------------------------------------
const SIGSUM_POLICY = {
  name: 'sigsum-generic-2025-1',
  sha256: '666d9d0b9ab2e4019769c42eaccd7d6d502a9abac45979bb9b08b7213e4f53e3',
  logs: [
    { name: 'seasalp.glasklar.is', keyHex: '0ec7e16843119b120377a73913ac6acbc2d03d82432e2c36b841b09a95841f25' },
    { name: 'ginkgo.tlog.mullvad.net', keyHex: 'f00c159663d09bbda6131ee1816863b6adcacfe80b0b288000b11aba8fe38314' },
  ],
  witnesses: [
    { name: 'witness.glasklar.is', keyHex: 'b2106db9065ec97f25e09c18839216751a6e26d8ed8b41e485a563d3d1498536' },
    { name: 'witness.mullvad.net', keyHex: '15d6d0141543247b74bab3c1076372d9c894f619c376d64b29aa312cc00f61ad' },
    { name: 'tillitis.se/tillitis-witness-1', keyHex: '076be8c9ee7ea60916f0df3608c945d7730082ecb37749dad2c9ed339fea770c' },
  ],
  quorum: 2,
};

const SUBJECT_LOG_POLICIES = {
  'ainumbers.co/registry/lineage': { keyHex: 'fe5eb9f2560469be7ff76e45fc695459db739a86117b659622cfc92513de13fa' },
  'ainumbers.co/registry/errata': { keyHex: 'c824d4cd8d69ac0251c74e420889a84997ff024e360d8cf80bd861f8db447be7' },
};

// The fixture-independent risc0 3.0.x ceremony constants (control_root,
// control_id, verifying key) — identical across every published receipt; the
// committed twin oracle carries them. Loaded lazily so importers of this
// module do not pay file I/O they may not need.
import { fileURLToPath } from 'node:url';
const __dirname = path.dirname(fileURLToPath(import.meta.url));
const ORACLE_CONSTANTS_PATH = path.resolve(
  __dirname, '..', '..', 'crates', 'zkprof-verify', 'fixtures', 'twin-oracle', 'art-01.oracle.json'
);
let oracleConstants = null;
function loadOracleConstants() {
  if (!oracleConstants) oracleConstants = JSON.parse(fs.readFileSync(ORACLE_CONSTANTS_PATH, 'utf8'));
  return oracleConstants;
}

// ---------------------------------------------------------------------------
// byte helpers (Buffer-free core so the halves stay portable; Node Buffer at
// the edges only)
// ---------------------------------------------------------------------------
function hexToBytes(hex) {
  const out = new Uint8Array(hex.length / 2);
  for (let i = 0; i < out.length; i++) out[i] = parseInt(hex.slice(2 * i, 2 * i + 2), 16);
  return out;
}
function bytesToHex(bytes) {
  let s = '';
  for (const b of bytes) s += b.toString(16).padStart(2, '0');
  return s;
}
function utf8(s) { return new TextEncoder().encode(s); }
function concat(parts) {
  const len = parts.reduce((n, p) => n + p.length, 0);
  const out = new Uint8Array(len);
  let off = 0;
  for (const p of parts) { out.set(p, off); off += p.length; }
  return out;
}
function bytesEqual(a, b) {
  if (a.length !== b.length) return false;
  for (let i = 0; i < a.length; i++) if (a[i] !== b[i]) return false;
  return true;
}

async function sha256(...parts) {
  return new Uint8Array(await subtle.digest('SHA-256', concat(parts)));
}
async function hashLeafNode(data) { return sha256(new Uint8Array([0x00]), data); }
async function hashInteriorNode(l, r) { return sha256(new Uint8Array([0x01]), l, r); }

async function importEd25519(hex) {
  return subtle.importKey('raw', hexToBytes(hex), { name: 'Ed25519' }, true, ['verify']);
}
async function ed25519Verify(pubKeyHex, sigHex, messageBytes) {
  const key = await importEd25519(pubKeyHex);
  return subtle.verify({ name: 'Ed25519' }, key, hexToBytes(sigHex), messageBytes);
}

// RFC 6962 §2.1.3.2 inclusion walk (iterative form; same node-ordering
// discipline as sigsum-go's merkle.VerifyInclusion: odd fn => sibling is the
// LEFT operand; fn < sn => sibling is the RIGHT operand).
async function verifyInclusion({ leafHash, index, size, rootHash, path }) {
  let r = leafHash;
  let fn = index;
  let p = path.slice();
  for (let sn = size - 1; sn > 0; fn = Math.floor(fn / 2), sn = Math.floor(sn / 2)) {
    if ((fn & 1) === 1) {
      if (!p.length) return false;
      r = await hashInteriorNode(p[0], r);
      p = p.slice(1);
    } else if (fn < sn) {
      if (!p.length) return false;
      r = await hashInteriorNode(r, p[0]);
      p = p.slice(1);
    }
  }
  return bytesEqual(r, rootHash);
}

// C2SP signed-note parsing (framing only): returns the note text every
// signature covers, plus the checkpoint's origin/size/root and each
// "— <name> <b64(keyHint‖sig)>" line split into name and signature bytes.
function parseSignedNote(text) {
  const lines = text.split('\n');
  if (lines.length < 4) throw new Error('signed-note: too few lines');
  const origin = lines[0];
  const size = Number(lines[1]);
  if (!Number.isInteger(size) || size < 0) throw new Error(`signed-note: malformed size: ${lines[1]}`);
  const rootB64 = lines[2];
  let i = 3;
  const extensionLines = [];
  while (i < lines.length && lines[i] !== '') { extensionLines.push(lines[i]); i++; }
  if (i >= lines.length) throw new Error('signed-note: no blank separator before signature block');
  const noteText = lines.slice(0, i).join('\n') + '\n';
  const signatures = [];
  for (let k = i + 1; k < lines.length; k++) {
    const line = lines[k];
    if (!line || !line.startsWith('— ')) continue;
    const parts = line.split(' ');
    const b64 = parts[2];
    const bin = Uint8Array.from(atob(b64), (c) => c.charCodeAt(0));
    if (bin.length < 4) throw new Error(`signed-note: signature line too short for a 4-byte key hint`);
    signatures.push({
      name: parts[1],
      keyIdHex: bytesToHex(bin.subarray(0, 4)),
      sigBytes: bin.subarray(4),
    });
  }
  return { origin, size, rootB64, extensionLines, noteText, signatures };
}

function base64ToBytes(b64) {
  const bin = atob(b64);
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out;
}

// ---------------------------------------------------------------------------
// PROOF half — thin adapter over the unchanged twin. Recomputes the §18.7
// journal bytes and digests; the bundle's journal_digest/claim_digest
// annotations are compared as a REPORTED DISCREPANCY only (never trusted).
// ---------------------------------------------------------------------------
export function verifyProofHalf(bundle) {
  const oracle = loadOracleConstants();
  const checks = [];
  const push = (name, passed, detail) =>
    checks.push({ name, passed: passed === true ? true : passed === false ? false : null, detail });

  const receipt = bundle?.receipt;
  if (!receipt || typeof receipt !== 'object') {
    push('receipt present', false, 'bundle.receipt missing or not an object');
    return { verdict: 'PROOF-INVALID', checks };
  }
  push('receipt present', true, `${receipt.type ?? '(no type)'} / ${receipt.system ?? '?'} / ${receipt.receiptFormat ?? '?'}`);

  let twin;
  try {
    twin = twinVerify(receipt, oracle, {});
  } catch (e) {
    push('twin mechanism', false, `threw: ${e.message}`);
    return { verdict: 'PROOF-INVALID', checks };
  }
  for (const c of twin.checks) push(c.name, c.passed, c.detail);

  // advisory-annotation cross-check (reported, never load-bearing)
  let derived = null;
  try { derived = deriveDigests(receipt); } catch { /* journal derivation already failed above */ }
  if (derived) {
    const annotatedJournal = bundle.journal_contract?.journal_digest;
    const annotatedClaim = bundle.journal_contract?.claim_digest;
    if (annotatedJournal !== undefined) {
      const matches = annotatedJournal === 'sha256:' + derived.journalDigestHex;
      push('journal_digest annotation matches recompute (ADVISORY)', matches,
        `bundle=${annotatedJournal} recomputed=sha256:${derived.journalDigestHex} — verdict decided by the recompute, not this annotation`);
    }
    if (annotatedClaim !== undefined) {
      const matches = annotatedClaim === 'sha256:' + derived.claimDigestHex;
      push('claim_digest annotation matches recompute (ADVISORY)', matches,
        `bundle=${annotatedClaim} recomputed=sha256:${derived.claimDigestHex}`);
    }
  }

  const verdict = twin.valid === true ? 'PROOF-VALID' : 'PROOF-INVALID';
  return { verdict, checks, journalDigestHex: derived?.journalDigestHex ?? null, claimDigestHex: derived?.claimDigestHex ?? null };
}

// ---------------------------------------------------------------------------
// LOGGED half — the Sigsum/C2SP chain, independently reimplemented.
// ---------------------------------------------------------------------------
export async function verifyLoggedHalf(bundle) {
  const checks = [];
  const push = (name, passed, detail) =>
    checks.push({ name, passed: passed === true ? true : passed === false ? false : null, detail });

  const anchor = bundle?.anchor;
  const record = anchor?.record;
  if (!record || typeof record !== 'object') {
    push('anchor record present', false, 'bundle.anchor.record missing or not an object');
    return { verdict: 'NOT-LOGGED', checks };
  }
  push('anchor record present', true, `type=${anchor.type ?? '(none)'} cosignatures=${(record.witness_cosignatures || []).length}`);

  // 1. trust policy is one THIS verifier pins (name + content hash)
  const tp = bundle.trust_policy;
  const policyPinned = tp?.name === SIGSUM_POLICY.name && tp?.sha256 === SIGSUM_POLICY.sha256;
  push('trust policy pinned by verifier', policyPinned,
    `bundle says name=${tp?.name} sha256=${tp?.sha256 ?? '(none)'}; verifier pins ${SIGSUM_POLICY.name} @ ${SIGSUM_POLICY.sha256} (2 logs, 3 witnesses, quorum ${SIGSUM_POLICY.quorum}-of-3)`);

  // 2. sha256(checkpoint_text) == anchored_hash
  let anchoredOk = false;
  let checkpoint = null;
  try {
    checkpoint = parseSignedNote(anchor.checkpoint_text);
    const cpDigest = await sha256(utf8(anchor.checkpoint_text));
    const anchoredBytes = hexToBytes(record.anchored_hash.replace(/^sha256:/, ''));
    anchoredOk = bytesEqual(cpDigest, anchoredBytes);
    push('anchored_hash == sha256(checkpoint_text)', anchoredOk,
      `anchored_hash=${record.anchored_hash}`);
  } catch (e) {
    push('anchored_hash == sha256(checkpoint_text)', false, `checkpoint_text unparseable: ${e.message}`);
  }

  // 3. checkpoint signature verifies vs the PINNED subject-log operator key
  let subjectOk = false;
  const subjectOrigin = anchor.subject?.origin;
  const pinnedSubject = SUBJECT_LOG_POLICIES[subjectOrigin];
  if (pinnedSubject && checkpoint) {
    try {
      const sigLine = checkpoint.signatures.find((s) => s.name === subjectOrigin) || checkpoint.signatures[0];
      subjectOk = await ed25519Verify(pinnedSubject.keyHex, bytesToHex(sigLine.sigBytes), utf8(checkpoint.noteText));
      push('checkpoint signature vs pinned subject-log key', subjectOk,
        `subject ${subjectOrigin} size ${checkpoint.size} — key pinned from ${anchor.subject?.log_policy ?? '(bundle subject.log_policy)'}`);
    } catch (e) {
      push('checkpoint signature vs pinned subject-log key', false, `error: ${e.message}`);
    }
  } else {
    push('checkpoint signature vs pinned subject-log key', false,
      `subject origin ${subjectOrigin ?? '(missing)'} is not a log this verifier pins`);
  }

  // 4. leaf commits to anchored_hash: checksum == sha256(anchored_hash bytes) + leaf signature
  let checksumOk = false;
  let leafSigOk = false;
  try {
    const message = hexToBytes(record.anchored_hash.replace(/^sha256:/, ''));
    const recomputed = await sha256(message);
    checksumOk = bytesToHex(recomputed) === record.leaf.checksum;
    push('leaf checksum == sha256(anchored_hash)', checksumOk, record.leaf.checksum);
    const leafSigData = concat([utf8('sigsum.org/v1/tree-leaf'), new Uint8Array([0]), hexToBytes(record.leaf.checksum)]);
    leafSigOk = await ed25519Verify(record.leaf.public_key, record.leaf.signature, leafSigData);
    push('leaf signature valid', leafSigOk, `submitter key ${record.leaf.public_key.slice(0, 16)}…`);
  } catch (e) {
    push('leaf commits to anchored_hash', false, `error: ${e.message}`);
  }

  // 5. RFC 6962 inclusion walk: leafHash(checksum‖signature‖keyHash) -> root
  let inclusionOk = false;
  try {
    const keyHash = await sha256(hexToBytes(record.leaf.public_key));
    const leafBin = concat([hexToBytes(record.leaf.checksum), hexToBytes(record.leaf.signature), keyHash]);
    const leafHash = await hashLeafNode(leafBin);
    const rootHash = hexToBytes(record.tree_head.root_hash);
    const path = (record.inclusion_proof.path || []).map((h) => hexToBytes(h));
    inclusionOk = await verifyInclusion({
      leafHash, index: record.inclusion_proof.leaf_index, size: record.tree_head.size, rootHash, path,
    });
    push('RFC 6962 inclusion proof -> tree_head root', inclusionOk,
      `leaf_index ${record.inclusion_proof.leaf_index} @ size ${record.tree_head.size} -> root ${record.tree_head.root_hash}`);
  } catch (e) {
    push('RFC 6962 inclusion proof -> tree_head root', false, `error: ${e.message}`);
  }

  // 6. the log's own signature over its checkpoint — origin DERIVED from the
  //    PINNED log key (never record.log_origin / record.log_public_key as
  //    authority: a pinned key must match the record's, or this fails)
  let logSigOk = false;
  let derivedOrigin = null;
  let matchedLog = null;
  try {
    matchedLog = SIGSUM_POLICY.logs.find(
      (l) => l.keyHex === record.log_public_key
    ) || null;
    if (!matchedLog) {
      push('log key is a pinned policy log', false,
        `record.log_public_key=${record.log_public_key} matches no pinned log in ${SIGSUM_POLICY.name}`);
    } else {
      derivedOrigin = 'sigsum.org/v1/tree/' + bytesToHex(await sha256(hexToBytes(matchedLog.keyHex)));
      const originMatches = derivedOrigin === record.log_origin;
      push('log key is a pinned policy log', originMatches,
        `${matchedLog.name}; derived origin ${derivedOrigin} ${originMatches ? '==' : '!='} record.log_origin`);
      const rootB64 = Buffer.from(hexToBytes(record.tree_head.root_hash)).toString('base64');
      const cpText = `${derivedOrigin}\n${record.tree_head.size}\n${rootB64}\n`;
      logSigOk = originMatches && await ed25519Verify(matchedLog.keyHex, record.tree_head.log_signature, utf8(cpText));
      push('log signature over checkpoint', logSigOk, `${matchedLog.name} @ size ${record.tree_head.size}`);
    }
  } catch (e) {
    push('log signature over checkpoint', false, `error: ${e.message}`);
  }

  // 7. witness quorum among the cosignatures — hash-match-before-pin, count
  //    only valid signatures from PINNED witnesses
  let witnessesOk = 0;
  const witnessDetail = [];
  try {
    const rootB64 = Buffer.from(hexToBytes(record.tree_head.root_hash)).toString('base64');
    const cpText = `${derivedOrigin ?? record.log_origin}\n${record.tree_head.size}\n${rootB64}\n`;
    const pinnedHashes = new Map();
    for (const w of SIGSUM_POLICY.witnesses) {
      pinnedHashes.set(bytesToHex(await sha256(hexToBytes(w.keyHex))), w);
    }
    for (const cs of record.witness_cosignatures || []) {
      const pinned = pinnedHashes.get(cs.key_hash);
      if (!pinned) { witnessDetail.push({ key_hash: cs.key_hash, pinned: false }); continue; }
      const cosData = utf8(`cosignature/v1\ntime ${cs.timestamp}\n${cpText}`);
      const ok = await ed25519Verify(pinned.keyHex, cs.signature, cosData);
      if (ok) witnessesOk++;
      witnessDetail.push({ name: pinned.name, pinned: true, valid: ok });
    }
    push('witness quorum met (pinned, valid cosignatures)', witnessesOk >= SIGSUM_POLICY.quorum,
      `${witnessesOk} valid pinned-witness cosignature(s) of ${(record.witness_cosignatures || []).length} carried; quorum ${SIGSUM_POLICY.quorum}-of-${SIGSUM_POLICY.witnesses.length} [${witnessDetail.filter((d) => d.valid).map((d) => d.name).join(', ')}]`);
  } catch (e) {
    push('witness quorum met (pinned, valid cosignatures)', false, `error: ${e.message}`);
  }

  const allPass = checks.every((c) => c.passed === true);
  return {
    verdict: allPass ? 'LOGGED' : 'NOT-LOGGED',
    checks,
    witnessSummary: { validPinned: witnessesOk, carried: (record.witness_cosignatures || []).length, quorum: SIGSUM_POLICY.quorum },
    subject: anchor.subject ?? null,
  };
}

// ---------------------------------------------------------------------------
// Combined verdict — the non-collapse rule made structural: `proof` and
// `logged` are SIBLINGS, each with its own verdict token; there is no field
// anywhere in this object that asserts both at once.
// ---------------------------------------------------------------------------
export async function verifyBundle(bundle) {
  const proof = verifyProofHalf(bundle);
  const logged = await verifyLoggedHalf(bundle);
  return {
    bundle_version: bundle?.bundle_version ?? null,
    proof: { verdict: proof.verdict, checks: proof.checks },
    logged: { verdict: logged.verdict, checks: logged.checks, subject: logged.subject },
    combined: {
      proof: proof.verdict === 'PROOF-VALID' ? 'VALID' : 'INVALID',
      logged: logged.verdict === 'LOGGED' ? 'VALID' : 'INVALID',
      note: 'two independent claims, stated separately; neither implies the other',
    },
  };
}

// ---------------------------------------------------------------------------
// CLI
// ---------------------------------------------------------------------------
function humanLine(result, file) {
  const lines = [];
  lines.push(`== ${file}`);
  lines.push(`bundle_version: ${result.bundle_version ?? '(missing)'}`);
  const proofFailed = result.proof.checks.filter((c) => c.passed === false).map((c) => c.name);
  const loggedFailed = result.logged.checks.filter((c) => c.passed === false).map((c) => c.name);
  lines.push(`PROOF HALF : ${result.proof.verdict}${proofFailed.length ? '  (failing: ' + proofFailed.join('; ') + ')' : ''}`);
  lines.push(`LOGGED HALF: ${result.logged.verdict}${loggedFailed.length ? '  (failing: ' + loggedFailed.join('; ') + ')' : ''}`);
  const subj = result.logged.subject;
  if (subj) {
    lines.push(`  log-half subject: ${subj.kind} ${subj.origin ?? ''} (policy ${subj.log_policy ?? '—'})`);
    lines.push(`  v0.1 scope: the log half commits to the anchored subject above, NOT to the receipt itself.`);
  }
  lines.push(`COMBINED   : proof=${result.combined.proof}, logged=${result.combined.logged} — ${result.combined.note}`);
  return lines.join('\n');
}

async function main() {
  const args = process.argv.slice(2);
  const jsonMode = args[0] === '--json';
  const files = (jsonMode ? args.slice(1) : args);
  if (!files.length) {
    console.error('usage: node web/bundle/verify-bundle.mjs [--json] <bundle.json> [...]');
    process.exit(2);
  }
  const results = [];
  let allGreen = true;
  for (const f of files) {
    const bundle = JSON.parse(fs.readFileSync(f, 'utf8'));
    const result = await verifyBundle(bundle);
    results.push({ file: path.basename(f), ...result });
    if (result.proof.verdict !== 'PROOF-VALID' || result.logged.verdict !== 'LOGGED') allGreen = false;
    if (!jsonMode) console.log(humanLine(result, path.basename(f)) + '\n');
  }
  if (jsonMode) console.log(JSON.stringify(results, null, 2));
  process.exit(allGreen ? 0 : 1);
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main().catch((e) => { console.error(e); process.exit(2); });
}
