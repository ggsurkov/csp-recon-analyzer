<script lang="ts">
  interface Props {
    onfile: (file: File) => void;
    ondemo: () => void;
    busy?: boolean;
    /** WASM is compiled and resident in the worker; analysis needs no network from here. */
    ready?: boolean;
    /** WASM could not be loaded at all. Nothing dropped here can be analysed. */
    initError?: string | null;
  }

  let { onfile, ondemo, busy = false, ready = false, initError = null }: Props = $props();

  /**
   * A failed compile is the only state that disables the zone.
   *
   * Not-yet-ready deliberately does not: the worker queues an incoming file behind its own
   * initialisation, so a drop in the first fraction of a second is handled correctly. Making
   * the controls inert for a window the user cannot perceive would only turn a working drop
   * into a silent no-op.
   */
  const disabled = $derived(busy || initError !== null);

  /**
   * Warm-up finishes in well under a second on a warm cache, and a status line that appears
   * and vanishes inside that window reads as a flicker, not as information. It is shown only
   * once the wait has lasted long enough to be worth explaining — the same reveal-delay
   * treatment the progress card gets.
   */
  const PENDING_REVEAL_DELAY_MS = 400;
  let showPending = $state(false);

  $effect(() => {
    if (ready || initError) {
      showPending = false;
      return;
    }
    const timer = setTimeout(() => (showPending = true), PENDING_REVEAL_DELAY_MS);
    return () => clearTimeout(timer);
  });

  let dragging = $state(false);
  let input: HTMLInputElement;

  /**
   * Nested elements fire dragleave as the pointer crosses them, which makes the highlight
   * flicker. Counting enter/leave pairs instead of toggling a boolean fixes it.
   */
  let dragDepth = 0;

  function take(files: FileList | null | undefined) {
    const file = files?.[0];
    if (file && !disabled) onfile(file);
  }

  function onDragEnter(event: DragEvent) {
    event.preventDefault();
    dragDepth += 1;
    dragging = true;
  }

  function onDragLeave(event: DragEvent) {
    event.preventDefault();
    dragDepth = Math.max(0, dragDepth - 1);
    if (dragDepth === 0) dragging = false;
  }

  function onDrop(event: DragEvent) {
    event.preventDefault();
    dragDepth = 0;
    dragging = false;
    take(event.dataTransfer?.files);
  }
</script>

<div
  class="rounded-2xl border-2 border-dashed p-10 text-center transition-colors
    {dragging ? 'border-emerald-400 bg-emerald-500/5' : 'border-slate-700 bg-slate-900/40'}
    {disabled ? 'pointer-events-none opacity-60' : ''}"
  role="region"
  aria-label="Reconciliation file drop zone"
  aria-busy={!ready && !initError}
  ondragenter={onDragEnter}
  ondragover={(e) => e.preventDefault()}
  ondragleave={onDragLeave}
  ondrop={onDrop}
>
  <div class="mx-auto max-w-xl space-y-4">
    <!--
      No busy state here any more: while an analysis runs, App swaps this whole component
      out for ProgressCard, which reports what is actually happening rather than spinning.
      `busy` survives only to disable the controls during the brief window before the
      progress card is revealed.
    -->
    <div class="text-4xl" aria-hidden="true">📄</div>

    <h2 class="text-lg font-semibold text-slate-100">Drop your reconciliation export here</h2>

    <p class="text-sm leading-relaxed text-slate-400">
      Partner Center &rarr; Billing &rarr; <span class="text-slate-300">Reconciliation</span>, the
      license-based file. <code class="rounded bg-slate-800 px-1.5 py-0.5 text-xs">.csv</code> or
      <code class="rounded bg-slate-800 px-1.5 py-0.5 text-xs">.csv.gz</code>, including the
      multi-blob gzip export.
    </p>

    <div class="flex flex-wrap items-center justify-center gap-3 pt-2">
      <button
        type="button"
        class="rounded-lg bg-emerald-500 px-4 py-2 text-sm font-semibold text-slate-950 transition hover:bg-emerald-400 disabled:opacity-50"
        {disabled}
        onclick={() => input.click()}
      >
        Choose a file
      </button>

      <button
        type="button"
        class="rounded-lg border border-slate-700 px-4 py-2 text-sm font-medium text-slate-300 transition hover:border-slate-500 hover:text-slate-100 disabled:opacity-50"
        {disabled}
        onclick={ondemo}
      >
        Try with the demo file
      </button>
    </div>

    <p class="pt-1 text-xs text-slate-500">
      The demo file is synthetic &mdash; 14 rows of invented tenants, built to exercise every
      edge case in the parser.
    </p>

    <!--
      The engine's state, stated plainly. "Ready" is worth saying out loud because it is the
      moment the offline promise on the badge above becomes literally true: from here the
      analyser is in memory and a file can be dropped with the network unplugged.
    -->
    <!-- Height reserved so the line arriving does not nudge the buttons above it. -->
    <p class="min-h-4 text-xs" aria-live="polite">
      {#if initError}
        <span class="text-red-300">
          The analyser engine could not be loaded: {initError} Reload the page while connected
          &mdash; once loaded it runs offline.
        </span>
      {:else if ready}
        <span class="text-emerald-400/80">
          Analyser loaded and running in this tab &mdash; you can disconnect from the network.
        </span>
      {:else if showPending}
        <span class="text-slate-500">Loading the analyser engine&hellip;</span>
      {/if}
    </p>

    <input
      bind:this={input}
      type="file"
      accept=".csv,.gz,.csv.gz,text/csv,application/gzip"
      class="hidden"
      onchange={(event) => {
        const el = event.currentTarget as HTMLInputElement;
        take(el.files);
        // Reset so choosing the same file twice still fires a change event.
        el.value = '';
      }}
    />
  </div>
</div>
