/**
 * Main-thread client for the analysis worker.
 *
 * One long-lived worker, keyed requests, so the WASM module is compiled once no matter how
 * many files the user drops.
 */
import type { AnalysisProgress, AnalysisResult } from './types';
import type { WorkerRequest, WorkerResponse } from './recon.worker';

type ProgressHandler = (progress: AnalysisProgress) => void;

type Pending = {
  resolve: (result: AnalysisResult) => void;
  reject: (error: Error) => void;
  onProgress?: ProgressHandler;
};

let worker: Worker | undefined;
let nextId = 0;
const pending = new Map<number, Pending>();

function getWorker(): Worker {
  if (worker) return worker;

  worker = new Worker(new URL('./recon.worker.ts', import.meta.url), { type: 'module' });

  worker.onmessage = (event: MessageEvent<WorkerResponse>) => {
    const message = event.data;
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
    for (const entry of pending.values()) entry.reject(error);
    pending.clear();
    worker?.terminate();
    worker = undefined;
  };

  return worker;
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
