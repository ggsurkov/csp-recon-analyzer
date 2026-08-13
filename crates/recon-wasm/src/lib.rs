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
//! One-shot, when the bytes are already in hand:
//!
//! ```js
//! import init, { parse_recon_bytes } from './wasm/recon_wasm.js';
//! await init();
//! const result = parse_recon_bytes(new Uint8Array(await file.arrayBuffer()));
//! console.log(result.total_est_leak_monthly.display, result.execution_time_ms);
//! ```
//!
//! Streaming, which is what the app uses. The file is pushed in chunk by chunk and never
//! exists as a JS `ArrayBuffer`, so a 1 GB export costs ~1 GB instead of ~2 GB:
//!
//! ```js
//! input_begin(file.size);
//! for await (const chunk of file.stream()) input_push(chunk);
//! const result = parse_input((p) => postMessage(p), 25000);
//! ```
//!
//! Run it in a Web Worker. Parsing is synchronous and CPU-bound; on the main thread a
//! large export freezes the tab.

pub mod report;

pub use report::{AnalysisResult, FindingView, Money, Phase, Progress, ANALYZER_VERSION};

#[cfg(target_arch = "wasm32")]
mod bindings {
    use std::cell::RefCell;

    use super::report;
    use wasm_bindgen::prelude::*;

    // The file being assembled, inside wasm linear memory.
    //
    // The alternative — read the whole `File` into a JS `ArrayBuffer` and hand that over —
    // holds the export twice at once: once on the JS heap and once in wasm, because `&[u8]`
    // arguments are copied across the boundary. At 1 GB that is the difference between
    // working and an out-of-memory tab. Here the bytes cross in chunks and only the
    // wasm-side copy is ever kept.
    thread_local! {
        static INPUT: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
    }

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
        to_js(&result)
    }

    /// Start a new streaming upload, reserving room for `expected_len` bytes.
    ///
    /// Any previous file is dropped *and its capacity released* first — analysing two large
    /// exports in a row must not need room for both. Returns an error rather than aborting
    /// the module when the allocation cannot be met, so an oversized file surfaces as a
    /// sentence in the UI instead of a dead worker.
    #[wasm_bindgen]
    pub fn input_begin(expected_len: usize) -> Result<(), JsValue> {
        INPUT.with(|cell| {
            let mut buf = cell.borrow_mut();
            buf.clear();
            buf.shrink_to_fit();
            buf.try_reserve_exact(expected_len).map_err(|_| {
                JsValue::from(js_sys::Error::new(&format!(
                    "This file needs {:.2} GB of memory and the browser tab could not \
                     provide it. Try the gzipped export, or split the file.",
                    expected_len as f64 / 1e9
                )))
            })
        })
    }

    /// Append one chunk. Copied straight from the JS typed array into the wasm buffer.
    #[wasm_bindgen]
    pub fn input_push(chunk: &js_sys::Uint8Array) {
        let len = chunk.length() as usize;
        INPUT.with(|cell| {
            let mut buf = cell.borrow_mut();
            let start = buf.len();
            // `resize` then `copy_to` is one memset plus one copy. Taking the chunk as
            // `&[u8]` instead would make wasm-bindgen copy it into a temporary first.
            buf.resize(start + len, 0);
            chunk.copy_to(&mut buf[start..]);
        });
    }

    /// Bytes accumulated so far. The read-side progress figure.
    #[wasm_bindgen]
    pub fn input_len() -> usize {
        INPUT.with(|cell| cell.borrow().len())
    }

    /// Drop the buffered file and release its memory. Call this if a read is abandoned.
    #[wasm_bindgen]
    pub fn input_clear() {
        INPUT.with(|cell| {
            let mut buf = cell.borrow_mut();
            buf.clear();
            buf.shrink_to_fit();
        });
    }

    /// Analyse the buffer built by [`input_begin`] / [`input_push`], reporting progress.
    ///
    /// `on_progress` is called with a plain object — `{bytesProcessed, totalBytes,
    /// rowsParsed, estFound, phase}` — at most once per `row_interval` rows, plus once per
    /// phase change. A throw inside it is swallowed: progress reporting must never be able
    /// to fail an analysis.
    ///
    /// The buffer is consumed. Whether this succeeds or fails, the bytes are released
    /// before returning, so a finished analysis does not sit on a gigabyte.
    #[wasm_bindgen]
    pub fn parse_input(
        on_progress: &js_sys::Function,
        row_interval: u32,
    ) -> Result<JsValue, JsValue> {
        let bytes = INPUT.with(|cell| std::mem::take(&mut *cell.borrow_mut()));

        let started = now_ms();
        let outcome = report::analyze_with_progress(&bytes, row_interval as u64, |p| {
            if let Ok(value) = serde_wasm_bindgen::to_value(p) {
                let _ = on_progress.call1(&JsValue::NULL, &value);
            }
        });
        drop(bytes);

        let mut result = outcome.map_err(|e| JsValue::from(js_sys::Error::new(&e)))?;
        result.execution_time_ms = now_ms() - started;
        to_js(&result)
    }

    fn to_js(result: &report::AnalysisResult) -> Result<JsValue, JsValue> {
        serde_wasm_bindgen::to_value(result).map_err(|e| {
            JsValue::from(js_sys::Error::new(&format!("Could not serialise the result: {e}")))
        })
    }

    /// High-resolution clock, falling back to `Date.now` where there is no `performance`.
    ///
    /// `Date::now` only has millisecond resolution, which reports "0 ms" for any file worth
    /// demoing, so it is the fallback rather than the default.
    fn now_ms() -> f64 {
        performance_now().unwrap_or_else(js_sys::Date::now)
    }

    /// `performance.now()`, reached through `globalThis`.
    ///
    /// Not via `web_sys::window()`: that is `None` inside a Worker, which is exactly where
    /// this crate runs. Reflecting off the global finds it wherever the module was loaded.
    fn performance_now() -> Option<f64> {
        let global = js_sys::global();
        let performance = js_sys::Reflect::get(&global, &JsValue::from_str("performance")).ok()?;
        if performance.is_undefined() || performance.is_null() {
            return None;
        }
        let now = js_sys::Reflect::get(&performance, &JsValue::from_str("now")).ok()?;
        now.dyn_ref::<js_sys::Function>()?.call0(&performance).ok()?.as_f64()
    }
}

#[cfg(target_arch = "wasm32")]
pub use bindings::*;
