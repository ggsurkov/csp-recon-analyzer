/// <reference lib="webworker" />
/**
 * Runs the WASM analyser off the main thread.
 *
 * Parsing is synchronous and CPU-bound. A 50k-line export on the main thread freezes the
 * tab for long enough that the browser offers to kill the page, so the work happens here
 * and the UI stays responsive.
 *
 * This worker makes exactly two network requests, both same-origin and both static assets
 * of this app: the WASM glue module and the `.wasm` binary. The recon bytes are passed in
 * from the page and never leave this thread.
 */
import init, { parse_recon_bytes } from './wasm/recon_wasm.js';
import wasmUrl from './wasm/recon_wasm_bg.wasm?url';

export interface WorkerRequest {
  id: number;
  bytes: ArrayBuffer;
}

export type WorkerResponse =
  | { id: number; ok: true; result: unknown }
  | { id: number; ok: false; error: string };

/** Instantiated once and reused; the module is ~330 KB and compiling it is not free. */
let ready: Promise<unknown> | undefined;

self.onmessage = async (event: MessageEvent<WorkerRequest>) => {
  const { id, bytes } = event.data;
  try {
    ready ??= init({ module_or_path: wasmUrl });
    await ready;
    const result = parse_recon_bytes(new Uint8Array(bytes));
    (self as DedicatedWorkerGlobalScope).postMessage({ id, ok: true, result } satisfies WorkerResponse);
  } catch (error) {
    // Rust throws a JS Error whose message is already a sentence meant for the user.
    const message = error instanceof Error ? error.message : String(error);
    (self as DedicatedWorkerGlobalScope).postMessage({ id, ok: false, error: message } satisfies WorkerResponse);
  }
};
