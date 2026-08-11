/* tslint:disable */
/* eslint-disable */

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
    readonly parse_recon_bytes: (a: number, b: number) => [number, number, number];
    readonly start: () => void;
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
