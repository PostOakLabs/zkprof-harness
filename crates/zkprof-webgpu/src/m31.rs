// SPDX-License-Identifier: Apache-2.0

//! The Mersenne-31 field, and a CPU mirror of the WGSL kernel.
//!
//! # Why this module exists
//!
//! [`mul`] is a line-for-line transcription of `m31_mul` in
//! `shaders/m31_mul.wgsl`, including the places where the shader relies on
//! `u32` arithmetic wrapping. That makes it the reference the GPU result is
//! checked against, and it means a disagreement between the two is a bug in
//! one of them rather than a difference in rounding.
//!
//! [`mul_reference`] is a second, independent implementation that does the
//! multiplication in `u64` and takes one remainder. It shares no steps with
//! the shader, so a bug in the limb arithmetic cannot hide behind it. The
//! tests cross the two against each other over the whole edge-case surface
//! and over a pseudorandom sweep.

/// The Mersenne-31 modulus, 2³¹ − 1.
///
/// Prime, and the reason the reduction in [`mul`] is a fold rather than a
/// division: 2³¹ ≡ 1, so a 62-bit product collapses in two shifts and a
/// conditional subtract.
pub const M31: u32 = 2_147_483_647;

/// The largest canonical field element, `M31 - 1`.
pub const M31_MAX: u32 = M31 - 1;

/// Multiplies in M31 using the same 32-bit limbs the WGSL kernel uses.
///
/// Both arguments are expected to be canonical field elements, i.e. at most
/// [`M31_MAX`]. Non-canonical inputs still return a value, but the bounds
/// that keep the intermediates inside a `u32` are proven for canonical ones,
/// so the result is not meaningful outside that range.
///
/// Every add and shift that mirrors a WGSL operation wraps, exactly as WGSL
/// defines it to. Rust's `+` traps on overflow in debug builds, so this uses
/// the wrapping forms deliberately: the point is to match the shader, and the
/// tests are what catch it if the matching is wrong.
#[must_use]
pub fn mul(a: u32, b: u32) -> u32 {
    let a_lo = a & 0xFFFF;
    let a_hi = a >> 16;
    let b_lo = b & 0xFFFF;
    let b_hi = b >> 16;

    let p0 = a_lo * b_lo;
    let p1 = a_hi * b_lo + a_lo * b_hi;
    let p2 = a_hi * b_hi;

    let mut lo = p0;
    let mut hi = p2;

    let mid_lo = (p1 & 0xFFFF) << 16;
    let mid_hi = p1 >> 16;

    let sum = lo.wrapping_add(mid_lo);
    hi = hi.wrapping_add(mid_hi).wrapping_add(u32::from(sum < lo));
    lo = sum;

    let t = (hi << 1).wrapping_add(lo >> 31).wrapping_add(lo & M31);
    let u = (t >> 31) + (t & M31);

    if u >= M31 {
        u - M31
    } else {
        u
    }
}

/// Multiplies in M31 the slow way: widen to `u64`, take one remainder.
///
/// Independent of [`mul`] by construction — no shared intermediate — so it
/// can serve as the arbiter in a cross-check. Available on every target
/// because `u64` is, which is what lets the cross-check run in CI without a
/// GPU.
#[must_use]
pub fn mul_reference(a: u32, b: u32) -> u32 {
    let product = u64::from(a) * u64::from(b);
    let reduced = product % u64::from(M31);
    // Exact rather than truncating: `reduced` is strictly below `M31`, which
    // is 2^31 - 1 and therefore fits in a u32 with room to spare.
    #[allow(clippy::cast_possible_truncation)]
    {
        reduced as u32
    }
}

/// Multiplies every element of `lhs` by the corresponding element of `rhs`.
///
/// The CPU reference for one dispatch. Used to check the GPU's output before
/// a report is allowed to exist, and to give the report something to compare
/// against that did not come from the device under test.
///
/// # Panics
///
/// Panics if the two slices differ in length. Both are produced by
/// [`crate::spec::Spec::inputs`], which cannot produce mismatched lengths, so
/// this is an invariant rather than a recoverable error.
#[must_use]
pub fn mul_all(lhs: &[u32], rhs: &[u32]) -> Vec<u32> {
    assert_eq!(
        lhs.len(),
        rhs.len(),
        "operand buffers must be the same length"
    );
    lhs.iter()
        .zip(rhs.iter())
        .map(|(a, b)| mul(*a, *b))
        .collect()
}

/// Reduces a `u64` to a canonical field element.
///
/// Used to turn pseudorandom generator output into field elements. Modulo
/// rather than masking: `M31` is not a power of two, so a mask would both
/// bias the distribution and leave the top element unreachable.
#[must_use]
pub fn from_u64(value: u64) -> u32 {
    let reduced = value % u64::from(M31);
    // Exact for the same reason as in `mul_reference`: the remainder of a
    // division by `M31` is strictly below 2^31.
    #[allow(clippy::cast_possible_truncation)]
    {
        reduced as u32
    }
}

#[cfg(test)]
mod tests {
    use super::{from_u64, mul, mul_all, mul_reference, M31, M31_MAX};

    // A small deterministic sweep. It is not a property test and does not
    // pretend to be: it covers the edges, plus a fixed sample of the interior
    // that is the same on every machine and in CI.
    const SWEEP: usize = 4096;

    fn splitmix64(state: &mut u64) -> u64 {
        *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = *state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    #[test]
    fn the_modulus_is_two_to_the_thirty_one_minus_one() {
        assert_eq!(M31, (1u32 << 31) - 1);
        assert_eq!(M31, 2_147_483_647);
        assert_eq!(M31_MAX, 2_147_483_646);
    }

    #[test]
    fn zero_and_one_behave_like_a_field() {
        for value in [0u32, 1, 2, M31_MAX] {
            assert_eq!(mul(value, 0), 0, "multiplying {value} by zero");
            assert_eq!(mul(0, value), 0, "multiplying zero by {value}");
            assert_eq!(mul(value, 1), value, "multiplying {value} by one");
            assert_eq!(mul(1, value), value, "multiplying one by {value}");
        }
    }

    #[test]
    fn the_largest_element_squares_correctly() {
        // M31 - 1 is -1 in the field, so its square is 1. This is the single
        // case that exercises the widest possible product, and it is the one
        // a limb-arithmetic bug shows up in first.
        assert_eq!(mul_reference(M31_MAX, M31_MAX), 1);
        assert_eq!(mul(M31_MAX, M31_MAX), 1);
    }

    #[test]
    fn the_limb_implementation_agrees_with_the_widened_one_on_the_edges() {
        let edges = [0u32, 1, 2, 0xFFFF, 0x1_0000, M31_MAX - 1, M31_MAX];
        for a in edges {
            for b in edges {
                assert_eq!(mul(a, b), mul_reference(a, b), "disagreement on {a} * {b}");
            }
        }
    }

    #[test]
    fn the_limb_implementation_agrees_with_the_widened_one_on_a_sweep() {
        let mut state = 0x5DEE_CE66_D9EF_2027u64;
        for _ in 0..SWEEP {
            let a = from_u64(splitmix64(&mut state));
            let b = from_u64(splitmix64(&mut state));
            assert_eq!(mul(a, b), mul_reference(a, b), "{a} * {b}");
        }
    }

    #[test]
    fn random_inputs_never_exceed_the_canonical_range() {
        let mut state = 0u64;
        for _ in 0..SWEEP {
            let value = from_u64(splitmix64(&mut state));
            assert!(value <= M31_MAX, "{value} is not a canonical element");
        }
    }

    #[test]
    fn a_product_never_lands_on_the_modulus() {
        // The fold produces a value below 2^31 + 1 and subtracts at most one
        // M31, so M31 itself must never survive. `mul_reference` cannot
        // return it either, and the two agreeing is the check.
        let mut state = 0x0123_4567_89AB_CDEFu64;
        for _ in 0..SWEEP {
            let a = from_u64(splitmix64(&mut state));
            let b = from_u64(splitmix64(&mut state));
            let product = mul(a, b);
            assert!(product < M31, "{a} * {b} produced {product}");
            assert_eq!(product, mul_reference(a, b));
        }
    }

    #[test]
    fn mul_all_is_elementwise() {
        let lhs = [1u32, 2, 3, M31_MAX];
        let rhs = [M31_MAX, 1, 0, M31_MAX];
        assert_eq!(
            mul_all(&lhs, &rhs),
            vec![mul(1, M31_MAX), mul(2, 1), mul(3, 0), mul(M31_MAX, M31_MAX)]
        );
    }

    #[test]
    #[should_panic(expected = "same length")]
    fn mul_all_refuses_mismatched_operands() {
        let _ = mul_all(&[1, 2, 3], &[1, 2]);
    }
}
