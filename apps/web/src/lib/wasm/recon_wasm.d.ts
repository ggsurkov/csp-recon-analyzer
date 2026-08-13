/* tslint:disable */
/* eslint-disable */

/**
 * Start a new streaming upload, reserving room for `expected_len` bytes.
 *
 * Any previous file is dropped *and its capacity released* first — analysing two large
 * exports in a row must not need room for both. Returns an error rather than aborting
 * the module when the allocation cannot be met, so an oversized file surfaces as a
 * sentence in the UI instead of a dead worker.
 */
export function input_begin(expected_len: number): void;

/**
 * Drop the buffered file and release its memory. Call this if a read is abandoned.
 */
export function input_clear(): void;

/**
 * Bytes accumulated so far. The read-side progress figure.
 */
export function input_len(): number;

/**
 * Append one chunk. Copied straight from the JS typed array into the wasm buffer.
 */
export function input_push(chunk: Uint8Array): void;

/**
 * Analyse the buffer built by [`input_begin`] / [`input_push`], reporting progress.
 *
 * `on_progress` is called with a plain object — `{bytesProcessed, totalBytes,
 * rowsParsed, estFound, phase}` — at most once per `row_interval` rows, plus once per
 * phase change. A throw inside it is swallowed: progress reporting must never be able
 * to fail an analysis.
 *
 * The buffer is consumed. Whether this succeeds or fails, the bytes are released
 * before returning, so a finished analysis does not sit on a gigabyte.
 */
export function parse_input(on_progress: Function, row_interval: number): any;

/**
 * Analyse a reconciliation export.
 *
 * `file_bytes` is the raw file: gzip (including the multi-member form Partner Center
 * produces) or plain CSV. The format is decided from the magic bytes, so a `.csv` that
 * is really gzip — which is what some mail gateways hand back — still works.
 *
 * Returns the [`report::AnalysisResult`] as a plain JS object. Throws a JS `Error`
 * carrying a sentence fit to show the user if the file cannot be read.
 */
export function parse_recon_bytes(file_bytes: Uint8Array): any;

/**
 * Installs a panic hook that prints a Rust backtrace to the browser console.
 *
 * Without it a panic surfaces as `unreachable executed`, which says nothing. Runs
 * automatically on module load.
 */
export function start(): void;

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
    readonly memory: WebAssembly.Memory;
    readonly input_begin: (a: number) => [number, number];
    readonly input_len: () => number;
    readonly input_push: (a: any) => void;
    readonly parse_input: (a: any, b: number) => [number, number, number];
    readonly parse_recon_bytes: (a: number, b: number) => [number, number, number];
    readonly start: () => void;
    readonly input_clear: () => void;
    readonly __wbindgen_malloc: (a: number, b: number) => number;
    readonly __wbindgen_realloc: (a: number, b: number, c: number, d: number) => number;
    readonly __wbindgen_exn_store: (a: number) => void;
    readonly __externref_table_alloc: () => number;
    readonly __wbindgen_externrefs: WebAssembly.Table;
    readonly __wbindgen_free: (a: number, b: number, c: number) => void;
    readonly __externref_table_dealloc: (a: number) => void;
    readonly __wbindgen_start: () => void;
}

export type SyncInitInput = BufferSource | WebAssembly.Module;

/**
 * Instantiates the given `module`, which can either be bytes or
 * a precompiled `WebAssembly.Module`.
 *
 * @param {{ module: SyncInitInput }} module - Passing `SyncInitInput` directly is deprecated.
 *
 * @returns {InitOutput}
 */
export function initSync(module: { module: SyncInitInput } | SyncInitInput): InitOutput;

/**
 * If `module_or_path` is {RequestInfo} or {URL}, makes a request and
 * for everything else, calls `WebAssembly.instantiate` directly.
 *
 * @param {{ module_or_path: InitInput | Promise<InitInput> }} module_or_path - Passing `InitInput` directly is deprecated.
 *
 * @returns {Promise<InitOutput>}
 */
export default function __wbg_init (module_or_path?: { module_or_path: InitInput | Promise<InitInput> } | InitInput | Promise<InitInput>): Promise<InitOutput>;
