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
 *
 * ## Why the file arrives as a `File` and not an `ArrayBuffer`
 *
 * A `File` is structured-cloned by reference — the bytes are not copied to get here. It is
 * then streamed into wasm a chunk at a time, so the export exists once, inside wasm linear
 * memory. Reading it into an `ArrayBuffer` first would hold it twice at the moment of the
 * call (JS heap plus the wasm-side copy `&[u8]` arguments make), which for the 1 GB stress
 * fixture is the difference between finishing and an out-of-memory worker.
 */
import init, { input_begin, input_clear, input_push, parse_input } from './wasm/recon_wasm.js';
import wasmUrl from './wasm/recon_wasm_bg.wasm?url';
import type { AnalysisProgress } from './types';

export interface WorkerRequest {
  id: number;
  /** The dropped file, streamed in here. Preferred: nothing is copied to send it. */
  file?: File;
  /** Pre-read bytes, for callers that already hold them (the bundled demo). Transferred. */
  bytes?: ArrayBuffer;
}

export type WorkerResponse =
  | ({ id: number; kind: 'progress' } & AnalysisProgress)
  | { id: number; kind: 'done'; result: unknown }
  | { id: number; kind: 'error'; error: string };

/**
 * Rows between progress ticks inside wasm.
 *
 * 25k rows is ~19 MB of the stress fixture: ~54 ticks across 1 GB, and roughly one every
 * 300 ms at the observed parse rate. Small enough that the bar moves, large enough that the
 * per-tick cost (build a JS object, cross the boundary) stays in the noise.
 */
const ROW_INTERVAL = 25_000;

/** Bytes per read chunk. Big enough to keep syscall overhead down, small enough to report often. */
const READ_CHUNK = 8 * 1024 * 1024;

/**
 * Floor on the gap between posted messages.
 *
 * The row interval bounds how often wasm *asks*; this bounds how often we answer. A file of
 * very short rows ticks far more often than 60 Hz, and every extra message is a structured
 * clone plus a task on the main thread's queue — the exact way a progress bar ends up
 * making the UI it decorates janky. Phase changes and the final tick always go through.
 */
const MIN_POST_INTERVAL_MS = 60;

/** Instantiated once and reused; the module is ~330 KB and compiling it is not free. */
let ready: Promise<unknown> | undefined;

const scope = self as DedicatedWorkerGlobalScope;

function post(response: WorkerResponse): void {
  scope.postMessage(response);
}

/**
 * Streams a `File` into the wasm-side buffer, reporting READING progress.
 *
 * Chunks are released as they are consumed — `reader.read()` hands over one `Uint8Array`,
 * it is copied into wasm, and the reference dies on the next iteration. Nothing accumulates
 * on the JS heap.
 */
function postRead(id: number, read: number, total: number): void {
  post({
    id,
    kind: 'progress',
    phase: 'READING',
    bytesProcessed: read,
    totalBytes: total,
    rowsParsed: 0,
    estFound: 0
  });
}

async function loadFile(id: number, file: File): Promise<void> {
  input_begin(file.size);
  postRead(id, 0, file.size);

  const reader = file.stream().getReader();
  let read = 0;
  let lastPost = 0;

  try {
    for (;;) {
      const { done, value } = await reader.read();
      if (done) break;

      // `file.stream()` picks its own chunk size; re-slicing to READ_CHUNK would copy for
      // no gain, so push what we are given and let the timer throttle the reporting.
      input_push(value);
      read += value.byteLength;

      const now = performance.now();
      if (now - lastPost >= MIN_POST_INTERVAL_MS) {
        lastPost = now;
        postRead(id, read, file.size);
      }
    }
  } finally {
    reader.releaseLock();
  }

  // The throttle will usually have swallowed the last chunk's tick. Send it unconditionally
  // so the read half of the bar always ends full rather than stopping a few percent short.
  postRead(id, read, file.size);
}

/**
 * Load bytes that are already in memory (the bundled demo).
 *
 * Still reports a READING phase, instantly complete. Every analysis then has the same
 * shape — read, then parse — which is what lets the progress bar map the two passes onto
 * one monotonic sweep without having to know which path it is on.
 */
function loadBytes(id: number, bytes: ArrayBuffer): void {
  input_begin(bytes.byteLength);
  postRead(id, 0, bytes.byteLength);
  for (let offset = 0; offset < bytes.byteLength; offset += READ_CHUNK) {
    input_push(new Uint8Array(bytes, offset, Math.min(READ_CHUNK, bytes.byteLength - offset)));
  }
  postRead(id, bytes.byteLength, bytes.byteLength);
}

scope.onmessage = async (event: MessageEvent<WorkerRequest>) => {
  const { id, file, bytes } = event.data;
  try {
    ready ??= init({ module_or_path: wasmUrl });
    await ready;

    if (file) {
      await loadFile(id, file);
    } else if (bytes) {
      loadBytes(id, bytes);
    } else {
      throw new Error('No file was supplied to the analyser.');
    }

    // Throttled here rather than in wasm: this side owns the message bus and is the only
    // one that can see the clock. `phase` changes bypass the throttle so the label never
    // lags behind what is actually happening.
    let lastPost = 0;
    let lastPhase = '';
    const onProgress = (p: AnalysisProgress) => {
      // wasm opens with a READING tick of its own — zero bytes, zero rows, "I have started".
      // Here that read has already happened above, so forwarding it would rewind the bar to
      // the start of a phase that is finished. The worker owns READING; wasm owns the rest.
      if (p.phase === 'READING') return;

      const now = performance.now();
      if (p.phase === lastPhase && now - lastPost < MIN_POST_INTERVAL_MS) return;
      lastPost = now;
      lastPhase = p.phase;
      post({ id, kind: 'progress', ...p });
    };

    const result = parse_input(onProgress, ROW_INTERVAL);
    post({ id, kind: 'done', result });
  } catch (error) {
    // Rust throws a JS Error whose message is already a sentence meant for the user.
    // Drop any partially-read file: without this an abandoned 1 GB read stays resident.
    input_clear();
    post({ id, kind: 'error', error: error instanceof Error ? error.message : String(error) });
  }
};
