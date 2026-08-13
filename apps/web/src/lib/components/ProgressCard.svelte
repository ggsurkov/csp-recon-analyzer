<script lang="ts">
  import type { AnalysisProgress, Phase } from '../types';

  interface Props {
    progress: AnalysisProgress | null;
    fileName: string;
  }

  let { progress, fileName }: Props = $props();

  const PHASE_LABEL: Record<Phase, string> = {
    READING: 'Reading the file',
    PARSING: 'Parsing rows and checking prices',
    EST_DETECTION: 'Rolling up per subscription',
    FINALIZING: 'Building the report'
  };

  /**
   * Completion comes from bytes, never from rows.
   *
   * The row total is unknown until the file ends, so a rows-based percentage would have to
   * invent its denominator and would lurch as the estimate moved. Bytes are exact.
   *
   * Every byte is handled twice — read in, then parsed — and each pass reports its own
   * `bytesProcessed / totalBytes`. Showing that figure raw makes the bar fill, snap back to
   * zero and fill again, which reads as a restart, so each pass gets half the bar. The
   * worker guarantees a READING phase even when the bytes were already in memory, which is
   * what keeps this a pure function of the latest tick: derive it from anything remembered
   * across ticks and the first frame renders before the memory is set, which shows up as
   * the bar jumping backwards.
   */
  const percent = $derived.by(() => {
    if (!progress || progress.totalBytes === 0) return 0;
    const fraction = Math.min(1, progress.bytesProcessed / progress.totalBytes);
    return progress.phase === 'READING' ? fraction * 50 : 50 + fraction * 50;
  });

  /**
   * Rows are extrapolated from how far through the bytes we are — so it is shown as `~`,
   * and only once there is enough of the file behind us for the average row length to have
   * settled. Below that the number would visibly lurch, which reads as a broken counter.
   */
  const estimatedTotalRows = $derived.by(() => {
    if (!progress || progress.rowsParsed === 0) return null;
    const fraction = progress.bytesProcessed / progress.totalBytes;
    if (fraction < 0.02 || fraction >= 1) return null;
    return Math.round(progress.rowsParsed / fraction);
  });

  const rowsLabel = $derived.by(() => {
    if (!progress) return '0';
    const parsed = progress.rowsParsed.toLocaleString();
    if (estimatedTotalRows === null) return parsed;
    return `${parsed} / ~${estimatedTotalRows.toLocaleString()}`;
  });

  function mb(bytes: number): string {
    return `${(bytes / 1_048_576).toLocaleString(undefined, { maximumFractionDigits: 1 })} MB`;
  }

  const bytesLabel = $derived(
    progress ? `${mb(progress.bytesProcessed)} / ${mb(progress.totalBytes)}` : ''
  );

  const phaseLabel = $derived(progress ? PHASE_LABEL[progress.phase] : PHASE_LABEL.READING);
  const estFound = $derived(progress?.estFound ?? 0);
</script>

<div
  class="rounded-2xl border border-slate-800 bg-slate-900/60 p-8"
  role="status"
  aria-live="polite"
  aria-label="Analysis progress"
>
  <div class="flex flex-wrap items-baseline justify-between gap-x-6 gap-y-1">
    <h2 class="text-lg font-semibold text-slate-100">Analysing</h2>
    <p class="truncate font-mono text-sm text-slate-500">{fileName}</p>
  </div>

  <div class="mt-6 flex items-baseline justify-between gap-4">
    <span class="text-sm text-slate-400">{phaseLabel}</span>
    <span class="tnum text-3xl font-semibold tracking-tight text-emerald-300">
      {percent.toFixed(percent >= 100 ? 0 : 1)}%
    </span>
  </div>

  <!--
    The width transition is what turns ~20 discrete ticks a second into a bar that appears
    to move continuously. Slightly longer than the message interval so each step is still
    animating when the next arrives.
  -->
  <div
    class="mt-3 h-2.5 overflow-hidden rounded-full bg-slate-800"
    role="progressbar"
    aria-valuemin={0}
    aria-valuemax={100}
    aria-valuenow={Math.round(percent)}
  >
    <div
      class="h-full rounded-full bg-gradient-to-r from-emerald-500 to-emerald-300 transition-[width] duration-200 ease-out"
      style:width="{percent}%"
    ></div>
  </div>

  <dl class="mt-6 grid grid-cols-2 gap-x-6 gap-y-4 sm:grid-cols-3">
    <div>
      <dt class="text-xs font-medium uppercase tracking-wider text-slate-500">Rows parsed</dt>
      <dd class="tnum mt-1 text-sm text-slate-200">{rowsLabel}</dd>
    </div>
    <div>
      <dt class="text-xs font-medium uppercase tracking-wider text-slate-500">Read</dt>
      <dd class="tnum mt-1 text-sm text-slate-200">{bytesLabel}</dd>
    </div>
    <div>
      <dt class="text-xs font-medium uppercase tracking-wider text-slate-500">EST lines found</dt>
      <dd
        class="tnum mt-1 text-sm font-semibold {estFound > 0 ? 'text-amber-300' : 'text-slate-400'}"
      >
        {estFound.toLocaleString()}
      </dd>
    </div>
  </dl>

  <p class="mt-6 text-xs leading-relaxed text-slate-500">
    Running in a Web Worker in this tab. No part of this file is uploaded — open the network
    panel and watch.
    {#if estimatedTotalRows !== null}
      The row total is estimated from how far through the file we are; the exact count is on the
      report.
    {/if}
  </p>
</div>
