// SPDX-License-Identifier: Apache-2.0
//
// Task 2 — the "snarkjs independent twin": an INDEPENDENT reimplementation of
// the risc0 3.0.5 Groth16/BN254 receipt verifier in pure JS (Node ESM), using
// ONLY the vendored noble bundle (@noble/curves + @noble/hashes v2.2.0) for
// the elliptic-curve/pairing arithmetic and SHA-256. It does NOT import or call
// the Rust `zkprof-verify` crate, and shares nothing with it at runtime except
// the receipt fixtures and the oracle dump JSON (a static data artifact).
//
// Everything below is reimplemented in JS from risc0's semantics:
//   * JCS (RFC 8785) canonical serialization of the journal object.
//   * the `ReceiptClaim::ok(...).digest()` tagged_struct construction.
//   * split_digest + the 5 Groth16 public inputs.
//   * the Groth16 pairing equation.
//
// Provenance of the CONSTANT ceremony parameters (control_root, control_id and
// the verifying key alpha/beta/gamma/delta/ic): these are the fixture-independent
// risc0 default parameters. control_root/control_id are read from the oracle
// dump and the vkey coordinates are read from the oracle dump too (both are
// constants identical across every fixture). The point COORDINATE
// convention is: G1 {x, y} and G2 {x_c0,x_c1,y_c0,y_c1}, acknowledged as the
// same extension basis in ark-bn254 and noble (verified via identical
// generator/b constants — see the oracle dump's README).
//
// The derivation semantics were understood from risc0 source:
//   - risc0-binfmt hash.rs:     tagged_struct / tagged_iter (see below).
//   - risc0-zkvm claim/receipt.rs: ReceiptClaim::ok + digest.
//   - risc0-groth16 verifier.rs: split_digest + the fr(control_id) step.
//   - risc0-groth16 verifier.rs: the default vkey constants (alpha/beta/gamma/
//     delta/ic) which the oracle dump reproduces.

import { bn254, sha256 } from './vendor/noble-bn254.bundle.mjs';

const BN254_R = BigInt(
  '21888242871839275222246405745257275088548364400416034343698204186575808495617'
);

const G1 = bn254.G1.Point;
const G2 = bn254.G2.Point;
const Fp12 = bn254.fields.Fp12;

// ---------------------------------------------------------------------------
// byte/hex/bigint helpers
// ---------------------------------------------------------------------------
const enc = new TextEncoder();

function hexToBytes(hex) {
  if (hex.length % 2) throw new Error('odd hex length');
  const out = new Uint8Array(hex.length / 2);
  for (let i = 0; i < out.length; i++) {
    out[i] = parseInt(hex.slice(2 * i, 2 * i + 2), 16);
  }
  return out;
}

function bytesToHex(bytes) {
  let s = '';
  for (const b of bytes) s += b.toString(16).padStart(2, '0');
  return s;
}

function bytesToBigIntBE(bytes) {
  let v = 0n;
  for (const b of bytes) v = (v << 8n) | BigInt(b);
  return v;
}

function bytesToBigIntLE(bytes) {
  let v = 0n;
  for (let i = 0; i < bytes.length; i++) v += BigInt(bytes[i]) << BigInt(8 * i);
  return v;
}

function u32le(w) {
  const b = new Uint8Array(4);
  new DataView(b.buffer).setUint32(0, w >>> 0, true);
  return b;
}

function u16le(w) {
  const b = new Uint8Array(2);
  new DataView(b.buffer).setUint16(0, w, true);
  return b;
}

function concatBytes(arr) {
  const len = arr.reduce((a, x) => a + x.length, 0);
  const out = new Uint8Array(len);
  let off = 0;
  for (const x of arr) {
    out.set(x, off);
    off += x.length;
  }
  return out;
}

function sha256Bytes(bytes) {
  return new Uint8Array(sha256(bytes));
}

// ---------------------------------------------------------------------------
// JCS (RFC 8785) canonicalization
// ---------------------------------------------------------------------------
// RFC 8785 number serialization caveat: a JSON number must serialize to the
// shortest representation that round-trips. JS `JSON.stringify` (ECMAScript
// Number::toString) and serde_json's default (Ryu) both produce shortest
// round-trip, so they agree for the finite journal numbers. The genuine
// edge cases (exact-exponent thresholds, `-0`, whole-valued floats that serde
// might render as "1.0" vs JS "1", etc.) are recorded as a caveat: none of the
// estate journals exercise them, and if one did the twin would flag a digest
// mismatch against the oracle rather than silently pass.

function jcsSerializeNumber(n) {
  return JSON.stringify(n);
}

// Sort keys by UTF-8 byte order to match Rust's BTreeMap<&str> (which orders by
// str byte order), not JS's default UTF-16 code-unit order.
function utf8KeyCompare(a, b) {
  const ba = enc.encode(a);
  const bb = enc.encode(b);
  const n = Math.min(ba.length, bb.length);
  for (let i = 0; i < n; i++) {
    if (ba[i] !== bb[i]) return ba[i] - bb[i];
  }
  return ba.length - bb.length;
}

function jcsSerializeString(s) {
  return JSON.stringify(s);
}

// Returns the RFC 8785 canonical serialization as a Uint8Array.
export function jcsCanonicalize(value) {
  const bytes = enc.encode(jcsWrite(value));
  return bytes;
}

function jcsWrite(value) {
  if (value === null) return 'null';
  if (typeof value === 'boolean') return value ? 'true' : 'false';
  if (typeof value === 'number') return jcsSerializeNumber(value);
  if (typeof value === 'string') return jcsSerializeString(value);
  if (Array.isArray(value)) {
    return '[' + value.map(jcsWrite).join(',') + ']';
  }
  if (typeof value === 'object') {
    const keys = Object.keys(value).sort(utf8KeyCompare);
    const parts = keys.map(
      (k) => jcsSerializeString(k) + ':' + jcsWrite(value[k])
    );
    return '{' + parts.join(',') + '}';
  }
  throw new Error('JCS: unsupported value ' + typeof value);
}

// ---------------------------------------------------------------------------
// risc0 tagged_struct / claim digest
// ---------------------------------------------------------------------------
function taggedStruct(tag, down, data) {
  const tagHash = sha256Bytes(enc.encode(tag));
  const downCount = u16le(down.length);
  return sha256Bytes(concatBytes([tagHash, ...down, ...data.map(u32le), downCount]));
}

// ReceiptClaim::ok(image_id, journal_bytes).digest()
export function claimDigest(imageIdHex, journalBytes) {
  const imageId = hexToBytes(imageIdHex);
  const ZERO = new Uint8Array(32);
  const journalHash = sha256Bytes(journalBytes);
  const post = taggedStruct('risc0.SystemState', [ZERO], [0]);
  const output = taggedStruct('risc0.Output', [journalHash, ZERO], []);
  return taggedStruct(
    'risc0.ReceiptClaim',
    [ZERO, imageId, post, output],
    [0, 0]
  );
}

// risc0 split_digest: reverse; a0 = fr(be[16..32]), a1 = fr(be[0..16]).
export function splitDigest(digestBytes) {
  const be = [...digestBytes].reverse();
  const a0 = bytesToBigIntBE(new Uint8Array(be.slice(16, 32)));
  const a1 = bytesToBigIntBE(new Uint8Array(be.slice(0, 16)));
  return [a0, a1];
}

// The 5th public input: id = LE(control_id) mod r, exactly risc0's
// fr(rev(control_id)) step.
export function controlIdFr(controlIdHex) {
  const bytes = hexToBytes(controlIdHex);
  return bytesToBigIntLE(bytes) % BN254_R;
}

function publicInputs(controlRootHex, controlIdHex, claimDigestBytes) {
  const [a0, a1] = splitDigest(hexToBytes(controlRootHex));
  const [c0, c1] = splitDigest(claimDigestBytes);
  const id = controlIdFr(controlIdHex);
  return [a0, a1, c0, c1, id];
}

// ---------------------------------------------------------------------------
// seal decode (independent)
// ---------------------------------------------------------------------------
// 256-byte Groth16 seal: A (G1: x,y BE), B (G2: x_c0,x_c1,y_c0,y_c1), C (G1).
export function decodeSeal(sealBytes) {
  if (sealBytes.length !== 256) {
    throw new Error(`seal length mismatch: ${sealBytes.length} != 256`);
  }
  const e = Uint8Array.from;
  const A = {
    x: bytesToBigIntBE(sealBytes.slice(0, 32)),
    y: bytesToBigIntBE(sealBytes.slice(32, 64)),
  };
  const b = sealBytes.slice(64, 192);
  const B = {
    x_c0: bytesToBigIntBE(b.slice(32, 64)), // data[96..128]
    x_c1: bytesToBigIntBE(b.slice(0, 32)), //  data[64..96]
    y_c0: bytesToBigIntBE(b.slice(96, 128)), // data[160..192]
    y_c1: bytesToBigIntBE(b.slice(64, 96)), // data[128..160]
  };
  const C = {
    x: bytesToBigIntBE(sealBytes.slice(192, 224)),
    y: bytesToBigIntBE(sealBytes.slice(224, 256)),
  };
  return { A, B, C };
}

// ---------------------------------------------------------------------------
// point construction
// ---------------------------------------------------------------------------
function g1pt(p) {
  return G1.fromAffine({ x: BigInt(p.x), y: BigInt(p.y) });
}
function g2pt(p) {
  return G2.fromAffine({
    x: { c0: BigInt(p.x_c0), c1: BigInt(p.x_c1) },
    y: { c0: BigInt(p.y_c0), c1: BigInt(p.y_c1) },
  });
}

// ---------------------------------------------------------------------------
// Groth16 pairing check
// ---------------------------------------------------------------------------
function pairingEqPublicInputs(
  A, B, C,
  alpha, beta, gamma, delta, ic,
  public_inputs
) {
  // ICsum = ic[0] + x0*ic[1] + ... + x4*ic[5]
  let icSum = g1pt(ic[0]);
  for (let i = 1; i <= 5; i++) {
    const scalar = public_inputs[i - 1];
    if (scalar !== 0n) {
      icSum = icSum.add(g1pt(ic[i]).multiply(scalar));
    }
  }
  const lhs = bn254.pairing(g1pt(A), g2pt(B));
  let rhs = bn254.pairing(g1pt(alpha), g2pt(beta));
  rhs = Fp12.mul(rhs, bn254.pairing(icSum, g2pt(gamma)));
  rhs = Fp12.mul(rhs, bn254.pairing(g1pt(C), g2pt(delta)));
  return Fp12.eql(lhs, rhs);
}

// Convenience: derive the JCS bytes, journal digest and claim digest for a
// receipt object (used by the driver / probes).
export function deriveDigests(receipt) {
  const jcsBytes = jcsCanonicalize(receipt.journal);
  const journalDigestHex = bytesToHex(sha256Bytes(jcsBytes));
  const d = claimDigest(receipt.imageId.slice(7), jcsBytes);
  return {
    jcsHex: bytesToHex(jcsBytes),
    journalDigestHex,
    claimDigestHex: bytesToHex(d),
  };
}

// ---------------------------------------------------------------------------
// Top-level verify
// ---------------------------------------------------------------------------
// receipt: { seal: b64, imageId: "sha256:..", journal: <object> }
// oracle:  { control_root, bn254_control_id, verifying_key, ... } (constants;
//          also carries reference claim_digest / public_inputs for cross-check)
// opts:    { expectedImageId?: string } — when present, imageId must equal it.
//
// Returns { valid, checks:[{name,passed,detail}] } mirroring the wasm verifier.
export function verify(receipt, oracle, opts = {}) {
  const checks = [];
  const push = (name, passed, detail) =>
    checks.push({ name, passed: passed === true ? true : passed === false ? false : null, detail });

  let sealBytes = null;
  // 1) seal decodes
  try {
    const b64 = receipt.seal;
    if (typeof b64 !== 'string') throw new Error('no seal field');
    sealBytes = new Uint8Array(Buffer.from(b64, 'base64'));
    if (sealBytes.length !== 256) throw new Error(`length ${sealBytes.length} != 256`);
    push('seal decodes', true, `${sealBytes.length} decoded bytes`);
  } catch (e) {
    push('seal decodes', false, `base64/length failed: ${e.message}`);
    return { valid: false, checks };
  }

  // 2) imageId recognized
  let imageId = receipt.imageId;
  let imageIdHex = null;
  let imageIdOk = false;
  if (typeof imageId === 'string' && imageId.startsWith('sha256:')) {
    imageIdHex = imageId.slice(7);
    imageIdOk = /^[0-9a-fA-F]{64}$/.test(imageIdHex);
  }
  const matchExpected =
    opts.expectedImageId === undefined || imageId === opts.expectedImageId;
  const imageIdPassed = imageIdOk && matchExpected;
  push(
    'imageId recognized',
    imageIdPassed,
    `imageId=${imageId} expected=${opts.expectedImageId ?? '(shape only)'}`
  );

  // 3) journal present
  let journal = receipt.journal;
  if (typeof journal !== 'object' || journal === null || Array.isArray(journal)) {
    push('journal present', false, 'journal is not an object');
    return { valid: false, checks };
  }
  push('journal present', true, 'object');

  // 4) claim digest derived
  let jcsBytes = null;
  let derived = null;
  try {
    jcsBytes = jcsCanonicalize(journal);
    const d = claimDigest(imageIdHex, jcsBytes);
    derived = bytesToHex(d);
    const matchesOracle = oracle?.claim_digest ? derived === oracle.claim_digest : null;
    push('claim digest derived', true, `sha256:${derived} oracleMatch=${matchesOracle}`);
  } catch (e) {
    push('claim digest derived', false, `error: ${e.message}`);
    return { valid: false, checks };
  }

  // 5) public inputs derived
  let pi = null;
  try {
    pi = publicInputs(
      oracle.control_root,
      oracle.bn254_control_id,
      hexToBytes(derived)
    );
    const ref = oracle.public_inputs?.map((s) => BigInt(s)) ?? null;
    const matchesOracle = ref
      ? ref.length === pi.length && ref.every((v, i) => v === pi[i])
      : null;
    push('public inputs derived', true, `[${pi.map((v) => v.toString()).join(', ')}] oracleMatch=${matchesOracle}`);
  } catch (e) {
    push('public inputs derived', false, `error: ${e.message}`);
    return { valid: false, checks };
  }

  // 6) Groth16 pairing check
  let proto = null;
  try {
    const { A, B, C } = decodeSeal(sealBytes);
    const vk = oracle.verifying_key;
    const ok = pairingEqPublicInputs(
      A, B, C,
      vk.alpha, vk.beta, vk.gamma, vk.delta, vk.ic,
      pi
    );
    // cross-check: the seal-decoded A/B/C must equal the oracle's proof coords
    const refProof = oracle.proof;
    const proofMatch =
      refProof &&
      BigInt(refProof.A.x) === A.x &&
      BigInt(refProof.A.y) === A.y &&
      BigInt(refProof.B.x_c0) === B.x_c0 &&
      BigInt(refProof.B.x_c1) === B.x_c1 &&
      BigInt(refProof.B.y_c0) === B.y_c0 &&
      BigInt(refProof.B.y_c1) === B.y_c1 &&
      BigInt(refProof.C.x) === C.x &&
      BigInt(refProof.C.y) === C.y;
    push('Groth16 pairing check', ok, `e(A,B)==e(α,β)·e(IC,γ)·e(C,δ) => ${ok}; sealDecode.matchOracle=${proofMatch}`);
  } catch (e) {
    push('Groth16 pairing check', false, `pairing failed: ${e.message}`);
  }

  const valid = checks.every((c) => c.passed === true);
  return { valid, checks };
}
