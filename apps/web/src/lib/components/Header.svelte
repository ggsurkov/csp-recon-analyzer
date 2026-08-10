<script lang="ts">
  /**
   * The locality badge is the product's central promise, so it says something checkable
   * rather than something reassuring: open the network tab and there is no upload.
   */
</script>

<header class="border-b border-slate-800 bg-slate-900/60 backdrop-blur">
  <div class="mx-auto flex max-w-7xl flex-wrap items-center gap-x-6 gap-y-3 px-6 py-4">
    <div class="flex items-center gap-3">
      <div
        class="grid h-9 w-9 place-items-center rounded-lg bg-emerald-500/15 text-lg ring-1 ring-emerald-500/30"
        aria-hidden="true"
      >
        🛡️
      </div>
      <div class="leading-tight">
        <div class="text-base font-semibold tracking-tight text-slate-50">ReconLedger</div>
        <div class="text-xs text-slate-400">Microsoft CSP reconciliation analyser</div>
      </div>
    </div>

    <div
      class="flex items-center gap-2 rounded-full border border-emerald-500/30 bg-emerald-500/10 px-3 py-1.5 text-xs font-medium text-emerald-300"
    >
      <span aria-hidden="true">🛡️</span>
      <span>100% Zero-Data-Exfiltration &mdash; Local WASM Execution</span>
    </div>

    <p class="ml-auto max-w-md text-xs leading-relaxed text-slate-500">
      Your file is read by WebAssembly inside this tab. There is no upload endpoint in this
      app &mdash; open your browser's network panel and confirm it.
    </p>
  </div>
</header>
