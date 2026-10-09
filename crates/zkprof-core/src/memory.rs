// SPDX-License-Identifier: Apache-2.0

//! Platform memory probes.
//!
//! One probe lives here, and it is deliberately narrow. [`linear_memory_bytes`]
//! reports the size of the wasm module's linear memory. That is not a heap
//! measurement in the usual sense, and the difference matters:
//!
//! | question                                    | answered here? |
//! |---------------------------------------------|----------------|
//! | How much memory did this workload allocate? | No             |
//! | How big did linear memory get?              | Yes            |
//!
//! The default Rust allocator on wasm32 grows linear memory and does not
//! shrink it, so the figure observed *after* a workload is a high-water mark
//! for the module as a whole. It includes everything allocated before the
//! workload started, which is why it is reported as one number with this
//! caveat attached rather than as a per-workload figure.
//!
//! Off `wasm32` there is no linear memory to measure, and this crate does not
//! pretend otherwise: the probe returns `None`. Reporting a native heap figure
//! would need an allocator hook this crate does not have, and guessing one
//! would be fabrication.

/// Size of the wasm linear memory, in bytes.
///
/// # What this is
///
/// `WebAssembly.Memory.buffer.byteLength` for the current module, read
/// reflectively. See the module docs for what it does and does not mean.
///
/// # Returns
///
/// `None` off `wasm32`, and on `wasm32` when the `WebAssembly.Memory` object
/// or its buffer cannot be reached. A missing measurement is reported as
/// absent rather than as zero, because zero reads as a measurement.
#[cfg(target_arch = "wasm32")]
#[must_use]
pub fn linear_memory_bytes() -> Option<u64> {
    use wasm_bindgen::JsCast;

    // `wasm_bindgen::memory()` hands back the module's `WebAssembly.Memory`.
    // `buffer` is read reflectively rather than through a generated binding
    // because js-sys exposes no `WebAssembly.Memory` type to hang one off.
    let memory = wasm_bindgen::memory();
    let buffer = js_sys::Reflect::get(&memory, &wasm_bindgen::JsValue::from_str("buffer")).ok()?;
    let buffer = buffer.dyn_into::<js_sys::ArrayBuffer>().ok()?;

    Some(u64::from(buffer.byte_length()))
}

/// Size of the wasm linear memory, in bytes. Always `None` off `wasm32`.
///
/// Exists so callers do not need a `cfg` block of their own at every call
/// site. See the module docs for why there is no native equivalent.
#[cfg(not(target_arch = "wasm32"))]
#[must_use]
pub fn linear_memory_bytes() -> Option<u64> {
    None
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_probe_reports_absence_rather_than_zero_off_wasm() {
        // On native there is no linear memory. The point of this test is the
        // non-wasm arm specifically: it must not grow a guess later.
        #[cfg(not(target_arch = "wasm32"))]
        assert_eq!(super::linear_memory_bytes(), None);
    }
}
