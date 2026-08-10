<script lang="ts">
  interface Props {
    onfile: (file: File) => void;
    ondemo: () => void;
    busy?: boolean;
  }

  let { onfile, ondemo, busy = false }: Props = $props();

  let dragging = $state(false);
  let input: HTMLInputElement;

  /**
   * Nested elements fire dragleave as the pointer crosses them, which makes the highlight
   * flicker. Counting enter/leave pairs instead of toggling a boolean fixes it.
   */
  let dragDepth = 0;

  function take(files: FileList | null | undefined) {
    const file = files?.[0];
    if (file && !busy) onfile(file);
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
    {busy ? 'pointer-events-none opacity-60' : ''}"
  role="region"
  aria-label="Reconciliation file drop zone"
  ondragenter={onDragEnter}
  ondragover={(e) => e.preventDefault()}
  ondragleave={onDragLeave}
  ondrop={onDrop}
>
  <div class="mx-auto max-w-xl space-y-4">
    <div class="text-4xl" aria-hidden="true">{busy ? '⏳' : '📄'}</div>

    <h2 class="text-lg font-semibold text-slate-100">
      {busy ? 'Analysing…' : 'Drop your reconciliation export here'}
    </h2>

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
        disabled={busy}
        onclick={() => input.click()}
      >
        Choose a file
      </button>

      <button
        type="button"
        class="rounded-lg border border-slate-700 px-4 py-2 text-sm font-medium text-slate-300 transition hover:border-slate-500 hover:text-slate-100 disabled:opacity-50"
        disabled={busy}
        onclick={ondemo}
      >
        Try with the demo file
      </button>
    </div>

    <p class="pt-1 text-xs text-slate-500">
      The demo file is synthetic &mdash; 14 rows of invented tenants, built to exercise every
      edge case in the parser.
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
