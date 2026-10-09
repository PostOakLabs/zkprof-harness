// SPDX-License-Identifier: Apache-2.0

//! The WGSL kernel, and the only check on it that runs without a device.
//!
//! # Why the shader is parsed and validated in a unit test
//!
//! A WGSL source file is compiled by the WebGPU implementation at run time.
//! Until then it is an unchecked string, and the first thing that would
//! notice a typo is a browser console on a machine with a GPU. That is the
//! slowest possible place to find out, and on the machine this crate was
//! built on it is not available at all.
//!
//! So the shader is parsed and validated by `naga` — the same front-end
//! `wgpu` itself uses — in an ordinary native test. That catches syntax
//! errors, type errors, and binding errors in CI, with no device involved.
//!
//! `naga` is pinned to the same version `wgpu` resolves to. A future
//! divergence would make the test validate against a different front-end than
//! the one the browser uses, which is worse than no test because it reports
//! success. The pin is in `Cargo.toml` and is the one dependency here whose
//! version is load-bearing rather than merely recorded.

/// The kernel source, embedded from `shaders/m31_mul.wgsl`.
///
/// Embedded rather than written inline so the file is the single place the
/// shader lives: an editor highlights it, `naga` parses it, and `wgpu`
/// compiles it, all from the same bytes.
pub const M31_MUL_WGSL: &str = include_str!("../shaders/m31_mul.wgsl");

/// The entry point `wgpu` is told to look for.
pub const ENTRY_POINT: &str = "main";

/// The shader's binding layout, as `wgpu` needs it spelled back.
///
/// Kept next to the source so the group and binding numbers are one table
/// rather than three numbers scattered through the pipeline code.
pub const BIND_GROUP: u32 = 0;
/// Binding for the left operand.
pub const BINDING_LHS: u32 = 0;
/// Binding for the right operand.
pub const BINDING_RHS: u32 = 1;
/// Binding for the product.
pub const BINDING_OUT: u32 = 2;

#[cfg(test)]
mod tests {
    use super::{ENTRY_POINT, M31_MUL_WGSL};
    use crate::spec::WORKGROUP_SIZE;

    /// Parses the kernel and runs `naga`'s validator over the result.
    ///
    /// Shared by the tests below so a failure names the stage it happened in.
    fn validated() -> naga::valid::ModuleInfo {
        let module =
            naga::front::wgsl::parse_str(M31_MUL_WGSL).expect("the kernel must parse as WGSL");

        let mut validator = naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::default(),
        );
        validator
            .validate(&module)
            .expect("the kernel must pass naga validation")
    }

    #[test]
    fn the_kernel_parses_and_validates() {
        let _info = validated();
    }

    #[test]
    fn the_shader_declares_the_entry_point_the_pipeline_names() {
        let module = naga::front::wgsl::parse_str(M31_MUL_WGSL).expect("parse");

        let entry = module
            .entry_points
            .iter()
            .find(|point| point.name == ENTRY_POINT)
            .unwrap_or_else(|| panic!("no entry point named {ENTRY_POINT:?}"));

        assert_eq!(
            entry.stage,
            naga::ShaderStage::Compute,
            "the entry point must be a compute stage"
        );
    }

    #[test]
    fn the_workgroup_size_matches_the_constant_the_host_dispatches_with() {
        // The host computes workgroup counts from `WORKGROUP_SIZE`, and the
        // shader fixes its own. If they disagree the run either leaves
        // elements unprocessed or indexes past a buffer, and neither is
        // reported by anything. The literal is read out of the source rather
        // than duplicated here.
        let needle = format!("@workgroup_size({WORKGROUP_SIZE})");
        assert!(
            M31_MUL_WGSL.contains(&needle),
            "the shader does not declare {needle:?}; it and WORKGROUP_SIZE must agree"
        );
    }

    #[test]
    fn the_kernel_declares_three_storage_bindings() {
        let module = naga::front::wgsl::parse_str(M31_MUL_WGSL).expect("parse");

        let storages = module
            .global_variables
            .iter()
            .filter(|(_, variable)| matches!(variable.space, naga::AddressSpace::Storage { .. }))
            .count();

        assert_eq!(
            storages, 3,
            "the kernel needs lhs, rhs and out; found {storages} storage bindings"
        );
    }

    #[test]
    fn the_source_is_not_empty_and_names_the_field() {
        // A guard against `include_str!` silently resolving to a stub: the
        // path is relative to this file, and a moved shader directory used to
        // be the kind of thing only the browser would have noticed.
        assert!(
            M31_MUL_WGSL.len() > 500,
            "the kernel source looks truncated"
        );
        assert!(M31_MUL_WGSL.contains("m31_mul"));
        assert!(M31_MUL_WGSL.contains("2147483647"));
    }
}
