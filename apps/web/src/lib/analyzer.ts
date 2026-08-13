/**
 * Main-thread client for the analysis worker.
 *
 * One long-lived worker, keyed requests, so the WASM module is compiled once no matter how
 * many files the user drops. Call {@link warmUpAnalyzer} as the page loads: spawning the
 * worker is itself a network request for its module, so leaving it until the first drop
 * would defeat the eager compile the worker does on start-up.
 */
import type { AnalysisProgress, AnalysisResult } from './types';
import type { WorkerRequest, WorkerResponse } from './recon.worker';

type ProgressHandler = (progress: AnalysisProgress) => void;

type Pending = {
  resolve: (result: AnalysisResult) => void;
  reject: (error: Error) => void;
  onProgress?: ProgressHandler;
};

type Deferred = {
  promise: Promise<void>;
  resolve: () => void;
  reject: (error: Error) => void;
};

let worker: Worker | undefined;
/** Settles when the worker reports its WASM module compiled, or that it could not be. */
let ready: Deferred | undefined;
let nextId = 0;
const pending = new Map<number, Pending>();

function defer(): Deferred {
  let resolve!: () => void;
  let reject!: (error: Error) => void;
  const promise = new Promise<void>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  // Warm-up is fire-and-forget for callers that do not care, so claim the rejection here.
  // Anyone who does await it still sees it; this only stops an unwatched failure from
  // surfacing as an unhandled rejection in the console.
  promise.catch(() => {});
  return { promise, resolve, reject };
}

function getWorker(): Worker {
  if (worker) return worker;

  ready = defer();
  worker = new Worker(new URL('./recon.worker.ts', import.meta.url), { type: 'module' });

  worker.onmessage = (event: MessageEvent<WorkerResponse>) => {
    const message = event.data;

    // Lifecycle messages belong to the worker as a whole, not to any one request, so they
    // are handled before the `id` lookup — they carry no `id` to look up.
    if (message.kind === 'ready') {
      ready?.resolve();
      return;
    }
    if (message.kind === 'init-error') {
      ready?.reject(new Error(message.error));
      return;
    }

    const entry = pending.get(message.id);
    if (!entry) return;

    // Progress does not settle the request, so the entry stays in the map.
    if (message.kind === 'progress') {
      const { id, kind, ...progress } = message;
      entry.onProgress?.(progress);
      return;
    }

    pending.delete(message.id);
    if (message.kind === 'done') {
      entry.resolve(message.result as AnalysisResult);
    } else {
      entry.reject(new Error(message.error));
    }
  };

  // A worker-level failure never resolves an individual request, so fail them all rather
  // than leaving the UI spinning forever.
  worker.onerror = (event) => {
    const error = new Error(event.message || 'The analyser worker stopped unexpectedly.');
    // A worker that failed to even load its module never sent `ready`; without this the
    // page would sit on "preparing" for the rest of the session. A no-op once settled.
    ready?.reject(error);
    for (const entry of pending.values()) entry.reject(error);
    pending.clear();
    worker?.terminate();
    worker = undefined;
    ready = undefined;
  };

  return worker;
}

/**
 * Spawn the worker and compile the WASM module now, while the page is loading.
 *
 * Resolves once the analyser is resident in worker memory and can run with the network
 * off; rejects if the module could not be loaded at all, which is the one case where a
 * later drop is guaranteed to fail and the UI should say so up front rather than waiting
 * for the user to hand over a file to find out.
 *
 * Idempotent, and safe to call before anything is dropped — it moves work earlier, it does
 * not duplicate it.
 */
export function warmUpAnalyzer(): Promise<void> {
  getWorker();
  return ready!.promise;
}

function submit(request: WorkerRequest, onProgress?: ProgressHandler, transfer: Transferable[] = []) {
  return new Promise<AnalysisResult>((resolve, reject) => {
    pending.set(request.id, { resolve, reject, onProgress });
    getWorker().postMessage(request, transfer);
  });
}

/**
 * Analyse the bytes of a `.csv` or `.csv.gz` reconciliation export.
 *
 * The buffer is transferred, not copied, so it is detached on this side once the call
 * returns. Prefer {@link analyzeFile} for anything the user picked: it hands the worker a
 * `File` and never materialises the bytes on this thread at all.
 */
export function analyzeBytes(bytes: ArrayBuffer, onProgress?: ProgressHandler): Promise<AnalysisResult> {
  const id = nextId++;
  return submit({ id, bytes }, onProgress, [bytes]);
}

/**
 * Analyse a file the user dropped or picked.
 *
 * The `File` is handed over as a reference — the worker streams it, so a 1 GB export is
 * never read into this thread's heap and the page stays interactive throughout.
 */
export function analyzeFile(file: File, onProgress?: ProgressHandler): Promise<AnalysisResult> {
  const id = nextId++;
  return submit({ id, file }, onProgress);
}
