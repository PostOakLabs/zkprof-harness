// SPDX-License-Identifier: Apache-2.0
//
// The W3.5 plumbing kernel: pointwise multiplication in the Mersenne-31 field.
//
// This is the whole point of the work unit. It is trivial on purpose: the job
// it has is to prove that a buffer can be filled, a shader dispatched, and a
// result read back and checked. Anything more interesting would make a
// plumbing failure look like a kernel bug.
//
// M31 is p = 2^31 - 1. It is the smallest field where the reduction is a fold
// rather than a division, which is why it shows up in STARK-friendly proving
// systems (Plonky2, Risc0, Winterfell all use it). It is also the reason this
// kernel needs the limb arithmetic below: WGSL has no 64-bit integer type, and
// the product of two 31-bit field elements is a 62-bit number.

const M31: u32 = 2147483647u; // 2^31 - 1

// The one function this kernel exists to exercise.
//
// It is written so that every intermediate fits in a u32 without overflow.
// The bounds are not incidental and each one is load-bearing, so they are
// spelled out:
//
//   a, b are canonical field elements, so a, b <= M31 - 1 = 2^31 - 2.
//   a_hi = a >> 16 <= 2^15 - 1,  b_hi = b >> 16 <= 2^15 - 1
//   a_lo = a & 0xFFFF <= 2^16 - 1, b_lo likewise
//
//   p0 = a_lo * b_lo   <  2^32
//   p1 = a_hi * b_lo + a_lo * b_hi
//          each term <= (2^15 - 1)(2^16 - 1) < 2^31, so the sum < 2^32
//   p2 = a_hi * b_hi   <= (2^15 - 1)^2 < 2^30
//
// Reassembling p2 * 2^32 + p1 * 2^16 + p0 into a (hi, lo) pair keeps the
// carry explicit: `sum < lo` detects the wrap of a u32 add, which WGSL
// defines as wrapping rather than as an error.
fn m31_mul(a: u32, b: u32) -> u32 {
  let a_lo = a & 0xFFFFu;
  let a_hi = a >> 16u;
  let b_lo = b & 0xFFFFu;
  let b_hi = b >> 16u;

  let p0 = a_lo * b_lo;
  let p1 = a_hi * b_lo + a_lo * b_hi;
  let p2 = a_hi * b_hi;

  var lo: u32 = p0;
  var hi: u32 = p2;

  let mid_lo = (p1 & 0xFFFFu) << 16u;
  let mid_hi = p1 >> 16u; // < 2^16, so `hi` below stays under 2^31

  let sum = lo + mid_lo;
  hi = hi + mid_hi + select(0u, 1u, sum < lo);
  lo = sum;

  // Now the product is hi * 2^32 + lo, and 2^32 = 2 (mod M31) because
  // 2^32 - (2^31 - 1) = 2^31 + 1 and 2^31 = 1 (mod M31). Folding one bit at
  // a time is what keeps this in 32 bits throughout:
  //
  //   hi < 2^30        =>  hi << 1 < 2^31
  //   lo >> 31 is 0 or 1
  //   lo & M31 <= 2^31 - 1
  //
  // so t <= 2^32 - 2 and the add cannot wrap. One more fold takes t under
  // 2^31 + 1, which a single conditional subtract finishes.
  let t = (hi << 1u) + (lo >> 31u) + (lo & M31);
  let u = (t >> 31u) + (t & M31);

  if (u >= M31) {
    return u - M31;
  }
  return u;
}

@group(0) @binding(0) var<storage, read> lhs: array<u32>;
@group(0) @binding(1) var<storage, read> rhs: array<u32>;
@group(0) @binding(2) var<storage, read_write> out: array<u32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
  let index = gid.x;
  // The dispatch covers whole workgroups, so the last one can run past the
  // end of the buffers. `arrayLength` is read from the binding rather than
  // passed in as a uniform, so the shader and the host cannot disagree about
  // how many elements there are.
  if (index >= arrayLength(&out)) {
    return;
  }
  out[index] = m31_mul(lhs[index], rhs[index]);
}
