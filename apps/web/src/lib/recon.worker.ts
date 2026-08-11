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

const scope = self as DedicatedWorkerGlobalScope;

function reply(response: WorkerResponse): void {
  scope.postMessage(response);
}

scope.onmessage = async (event: MessageEvent<WorkerRequest>) => {
  const { id, bytes } = event.data;
  try {
    ready ??= init({ module_or_path: wasmUrl });
    await ready;
    reply({ id, ok: true, result: parse_recon_bytes(new Uint8Array(bytes)) });
  } catch (error) {
    // Rust throws a JS Error whose message is already a sentence meant for the user.
    reply({ id, ok: false, error: error instanceof Error ? error.message : String(error) });
  }
};
