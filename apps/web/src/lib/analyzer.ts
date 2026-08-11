/**
 * Main-thread client for the analysis worker.
 *
 * One long-lived worker, keyed requests, so the WASM module is compiled once no matter how
 * many files the user drops.
 */
import type { AnalysisResult } from './types';
import type { WorkerRequest, WorkerResponse } from './recon.worker';

type Pending = {
  resolve: (result: AnalysisResult) => void;
  reject: (error: Error) => void;
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
    pending.delete(message.id);
    if (message.ok) {
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

/**
 * Analyse the bytes of a `.csv` or `.csv.gz` reconciliation export.
 *
 * The buffer is transferred, not copied, so it is detached on this side once the call
 * returns. Read anything you still need from the `File` before calling.
 */
export function analyzeBytes(bytes: ArrayBuffer): Promise<AnalysisResult> {
  const id = nextId++;
  const request: WorkerRequest = { id, bytes };
  return new Promise<AnalysisResult>((resolve, reject) => {
    pending.set(id, { resolve, reject });
    getWorker().postMessage(request, [bytes]);
  });
}

export async function analyzeFile(file: File): Promise<AnalysisResult> {
  return analyzeBytes(await file.arrayBuffer());
}
