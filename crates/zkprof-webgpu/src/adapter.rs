// SPDX-License-Identifier: Apache-2.0

//! How a WebGPU adapter is described in a report.
//!
//! `target.gpu` is the one field in a `zkprof-report` that names hardware a
//! browser chose on our behalf. It is also the one a reader is most likely to
//! generalise from: "this number came from an M1" travels a lot further than
//! the number it was attached to. So the string is built here, in code that
//! runs in CI, rather than inline at the point of use in code that only runs
//! on a machine with a GPU.

use zkprof_core::model::{Target, TargetKind};

/// What goes in place of an adapter that did not report a name.
///
/// Not a guess at what the adapter is. It says the adapter declined to say,
/// which is the observation that was actually made — the difference matters
/// because a reader who sees a name will use it, and a reader who sees this
/// knows there is no name to use.
pub const NO_NAME: &str = "(no name reported)";

/// Composes the `target.gpu` string.
///
/// The backend and device type are included even when a name is present,
/// because a name alone is not enough to place a number: `Apple M1 Pro`
/// running through the Metal backend and the same string through a
/// translation layer are not the same measurement, and only one of the three
/// parts says which it was.
///
/// All three arguments are strings rather than `wgpu` types so this function
/// is ordinary Rust and is tested without a device. The caller does the
/// formatting of the `wgpu` values with `{}` and `{:?}`.
#[must_use]
pub fn adapter_string(name: &str, backend: &str, device_type: &str) -> String {
    let name = if name.is_empty() { NO_NAME } else { name };
    format!("{name} [{backend} / {device_type}]")
}

/// Fills in the WebGPU-specific fields of a target the browser described.
///
/// `browser`, `os` and `cpu` come from [`zkprof_core::browser::browser_target`]
/// and are passed through untouched; only `kind` and `gpu` change. The
/// browser's own description of itself is not a WebGPU fact and is not
/// rewritten here.
///
/// An empty adapter string leaves `gpu` absent rather than emitting an empty
/// one: the schema requires `minLength: 1`, and absent is the honest encoding
/// of "nothing was reported".
#[must_use]
pub fn webgpu_target(base: Target, adapter: &str) -> Target {
    let gpu = if adapter.is_empty() {
        None
    } else {
        Some(adapter.to_owned())
    };
    Target {
        kind: TargetKind::Webgpu,
        gpu,
        ..base
    }
}

#[cfg(test)]
mod tests {
    use super::{adapter_string, webgpu_target, NO_NAME};
    use zkprof_core::model::{Target, TargetKind};

    fn browser_target() -> Target {
        Target {
            kind: TargetKind::WasmScalar,
            browser: Some("Chrome 141".to_owned()),
            os: "Mac OS X".to_owned(),
            cpu: "wasm32".to_owned(),
            gpu: None,
        }
    }

    #[test]
    fn a_named_adapter_keeps_all_three_parts() {
        let string = adapter_string("Apple M1 Pro", "Metal", "DiscreteGpu");
        assert_eq!(string, "Apple M1 Pro [Metal / DiscreteGpu]");
    }

    #[test]
    fn an_unnamed_adapter_says_so_rather_than_guessing() {
        let string = adapter_string("", "Vulkan", "IntegratedGpu");
        assert!(string.starts_with(NO_NAME), "{string}");
        assert!(
            string.contains("Vulkan") && string.contains("IntegratedGpu"),
            "{string}"
        );
    }

    #[test]
    fn the_string_is_never_empty() {
        // The schema requires `minLength: 1` on `target.gpu`, so an empty
        // string would produce a report the validator rejects.
        for (name, backend, kind) in [("", "", ""), ("name", "", ""), ("", "Vulkan", "")] {
            let string = adapter_string(name, backend, kind);
            assert!(!string.is_empty(), "{name:?}/{backend:?}/{kind:?}");
        }
    }

    #[test]
    fn the_target_kind_becomes_webgpu() {
        let target = webgpu_target(browser_target(), "Some Adapter [Metal / DiscreteGpu]");
        assert_eq!(target.kind, TargetKind::Webgpu);
        assert_eq!(
            target.gpu.as_deref(),
            Some("Some Adapter [Metal / DiscreteGpu]")
        );
    }

    #[test]
    fn the_browsers_own_description_is_passed_through_untouched() {
        let target = webgpu_target(browser_target(), "adapter");
        assert_eq!(target.browser.as_deref(), Some("Chrome 141"));
        assert_eq!(target.os, "Mac OS X");
        assert_eq!(target.cpu, "wasm32");
    }

    #[test]
    fn an_empty_adapter_leaves_gpu_absent() {
        let target = webgpu_target(browser_target(), "");
        assert!(
            target.gpu.is_none(),
            "an empty adapter must be absent, not an empty string"
        );
    }

    #[test]
    fn the_kind_is_the_schema_spelling() {
        let target = webgpu_target(browser_target(), "adapter");
        assert_eq!(target.kind.as_str(), "webgpu");
        assert_eq!(
            target.kind.as_str().parse::<TargetKind>().unwrap(),
            TargetKind::Webgpu
        );
    }
}
