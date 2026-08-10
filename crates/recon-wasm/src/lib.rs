//! # recon-wasm
//!
//! WebAssembly bindings for [`recon_core`]. One exported function: hand it the bytes of a
//! reconciliation export, get back the analysis.
//!
//! ## The promise this crate has to keep
//!
//! The product claim is that the file never leaves the machine. That claim is only as good
//! as the code behind it, so: **there is no networking in this crate, and none in anything
//! it depends on.** `recon-core` pulls in `csv`, `flate2`, `chrono`, `rust_decimal` and
//! `serde` — none of which opens a socket. The bindings below use `js-sys` to read the
//! clock and `serde-wasm-bindgen` to convert a struct. Nothing here can send anything
//! anywhere, and a reviewer can confirm that from the dependency list alone.
//!
//! The page that hosts this ships a Content-Security-Policy pinning `connect-src` to
//! `'self'`, so even a compromised dependency has nowhere to send data to.
//!
//! ## Usage from JavaScript
//!
//! ```js
//! import init, { parse_recon_bytes } from './wasm/recon_wasm.js';
//! await init();
//! const result = parse_recon_bytes(new Uint8Array(await file.arrayBuffer()));
//! console.log(result.total_est_leak_monthly.display, result.execution_time_ms);
//! ```
//!
//! Run it in a Web Worker. Parsing is synchronous and CPU-bound; on the main thread a
//! large export freezes the tab.

pub mod report;

pub use report::{AnalysisResult, FindingView, Money};

#[cfg(target_arch = "wasm32")]
mod bindings {
    use super::report;
    use wasm_bindgen::prelude::*;

    /// Installs a panic hook that prints a Rust backtrace to the browser console.
    ///
    /// Without it a panic surfaces as `unreachable executed`, which says nothing. Runs
    /// automatically on module load.
    #[wasm_bindgen(start)]
    pub fn start() {
        console_error_panic_hook::set_once();
    }

    /// Analyse a reconciliation export.
    ///
    /// `file_bytes` is the raw file: gzip (including the multi-member form Partner Center
    /// produces) or plain CSV. The format is decided from the magic bytes, so a `.csv` that
    /// is really gzip — which is what some mail gateways hand back — still works.
    ///
    /// Returns the [`report::AnalysisResult`] as a plain JS object. Throws a JS `Error`
    /// carrying a sentence fit to show the user if the file cannot be read.
    #[wasm_bindgen]
    pub fn parse_recon_bytes(file_bytes: &[u8]) -> Result<JsValue, JsValue> {
        let started = now_ms();
        let mut result =
            report::analyze(file_bytes).map_err(|e| JsValue::from(js_sys::Error::new(&e)))?;
        result.execution_time_ms = now_ms() - started;

        serde_wasm_bindgen::to_value(&result).map_err(|e| {
            JsValue::from(js_sys::Error::new(&format!("Could not serialise the result: {e}")))
        })
    }

    /// Version of this build, so the UI can show what it is running.
    #[wasm_bindgen]
    pub fn version() -> String {
        env!("CARGO_PKG_VERSION").to_owned()
    }

    /// High-resolution clock, reached through `globalThis`.
    ///
    /// `web_sys::window()` is `None` inside a Worker and `Date::now()` only has millisecond
    /// resolution, which reports "0 ms" for any file worth demoing. This finds
    /// `performance.now` wherever the module happens to be running and falls back to
    /// `Date.now` if it is absent.
    fn now_ms() -> f64 {
        let global = js_sys::global();
        let performance = js_sys::Reflect::get(&global, &JsValue::from_str("performance"));
        if let Ok(performance) = performance {
            if !performance.is_undefined() && !performance.is_null() {
                if let Ok(now) = js_sys::Reflect::get(&performance, &JsValue::from_str("now")) {
                    if let Some(now) = now.dyn_ref::<js_sys::Function>() {
                        if let Some(ms) = now.call0(&performance).ok().and_then(|v| v.as_f64()) {
                            return ms;
                        }
                    }
                }
            }
        }
        js_sys::Date::now()
    }
}

#[cfg(target_arch = "wasm32")]
pub use bindings::*;
