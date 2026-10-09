// SPDX-License-Identifier: Apache-2.0

//! Which `Target` a build reports, and what it can honestly see about the host.
//!
//! # Two different questions
//!
//! `target.kind` is a fact about *this build*, knowable at compile time:
//! was it compiled for wasm, was `+simd128` on, was the threads feature on.
//! Nothing at runtime can change it.
//!
//! `target.browser`, `target.os` and `target.cpu` are facts about *the host*,
//! which the compiled code cannot see and the browser will only volunteer in
//! the form of two strings. [`browser_target`] copies those strings verbatim.
//! It does not parse them.
//!
//! That is a deliberate refusal. User-agent strings are a negotiation, not a
//! description: they contain compatibility tokens for browsers that are not
//! running, and any parser is a set of guesses about a format designed to be
//! lied in. A report that says `os: "Linux x86_64"` because `navigator.platform`
//! said so is reporting an observation; a report that says `os: "linux"` because
//! a parser recognised a token is reporting an inference. This crate only ever
//! does the first.

use crate::model::TargetKind;
// Only [`browser_target`] builds one, and that is wasm32-only.
#[cfg(target_arch = "wasm32")]
use crate::model::Target;

/// The `target.kind` this module reports.
///
/// Decided entirely from `cfg`, so it is a property of the build and not of
/// the machine that runs it. When both `web-threads` and `simd128` apply the
/// threads kind wins, because `wasm-threads` is the more specific description
/// of what was compiled, and the simd flag stays visible on its own through
/// the exported `simd_enabled`.
#[must_use]
pub fn build_target_kind() -> TargetKind {
    if cfg!(not(target_arch = "wasm32")) {
        return TargetKind::Native;
    }
    if cfg!(feature = "web-threads") {
        return TargetKind::WasmThreads;
    }
    if cfg!(target_feature = "simd128") {
        TargetKind::WasmSimd
    } else {
        TargetKind::WasmScalar
    }
}

/// How this module describes its own CPU.
///
/// The architecture it was compiled for, plus the one wasm target feature that
/// changes the answer to every performance question. On a native build this
/// follows [`crate::model::Target::native`] and reports the host
/// architecture; it is a description of what was compiled, not a probe of what
/// is running.
#[must_use]
pub fn cpu_description() -> String {
    let mut cpu = std::env::consts::ARCH.to_owned();
    if cfg!(target_feature = "simd128") {
        cpu.push_str(" +simd128");
    }
    cpu
}

/// What the browser will say about itself, assembled into a [`Target`].
///
/// Every field that cannot be observed is left absent rather than filled with
/// a default: an absent `browser` reads as "not measured", whereas
/// `browser: "unknown"` reads as a measurement that came back strange.
///
/// The strings are copied unparsed. See the module docs.
#[cfg(target_arch = "wasm32")]
#[must_use]
pub fn browser_target() -> Target {
    let (browser, os) = match web_sys::window() {
        // Both getters are fallible on their own terms, so nothing here
        // unwraps: a browser that declines to answer yields `None`, which
        // serialises as an absent field.
        Some(window) => {
            let navigator = window.navigator();
            (navigator.user_agent().ok(), navigator.platform().ok())
        }
        // Outside a window scope -- a worker -- there is no `window.navigator`.
        // `userAgent` and `platform` live on `WorkerNavigator` too, reached
        // through `globalThis.navigator`, so fall back rather than reporting
        // nothing.
        None => (
            global_navigator_string("userAgent"),
            global_navigator_string("platform"),
        ),
    };

    Target {
        kind: build_target_kind(),
        browser,
        os: os.unwrap_or_else(|| "not reported by the host".to_owned()),
        cpu: cpu_description(),
        gpu: None,
    }
}

/// Reads a string property off `globalThis.navigator`.
///
/// Used for worker scopes, where `web_sys::window()` is `None` but a
/// `WorkerNavigator` with the same two properties is still present.
#[cfg(target_arch = "wasm32")]
fn global_navigator_string(property: &str) -> Option<String> {
    let navigator = js_sys::Reflect::get(
        &js_sys::global(),
        &wasm_bindgen::JsValue::from_str("navigator"),
    )
    .ok()?;
    let value =
        js_sys::Reflect::get(&navigator, &wasm_bindgen::JsValue::from_str(property)).ok()?;
    value.as_string()
}

#[cfg(test)]
mod tests {
    use super::{build_target_kind, cpu_description};
    use crate::model::TargetKind;

    #[test]
    fn a_native_build_reports_the_native_kind() {
        // This test only says anything on a native host, which is where the
        // workspace test run happens. It pins the rule that a non-wasm build
        // never claims to be a browser target.
        #[cfg(not(target_arch = "wasm32"))]
        assert_eq!(build_target_kind(), TargetKind::Native);
    }

    #[test]
    fn the_kind_is_one_of_the_two_wasm_spellings_on_wasm() {
        #[cfg(target_arch = "wasm32")]
        assert!(matches!(
            build_target_kind(),
            TargetKind::WasmScalar | TargetKind::WasmSimd | TargetKind::WasmThreads
        ));
    }

    #[test]
    fn the_cpu_description_names_the_compiled_architecture() {
        assert!(
            cpu_description().starts_with(std::env::consts::ARCH),
            "cpu description {:?} should start with the compiled arch {:?}",
            cpu_description(),
            std::env::consts::ARCH
        );
    }

    #[test]
    fn a_wasm_target_kind_is_a_schema_spelling() {
        for kind in [
            TargetKind::WasmScalar,
            TargetKind::WasmSimd,
            TargetKind::WasmThreads,
        ] {
            // Round-tripping catches a kind whose `as_str` has drifted from
            // what the schema allows.
            assert_eq!(kind.as_str().parse::<TargetKind>().unwrap(), kind);
        }
    }
}
