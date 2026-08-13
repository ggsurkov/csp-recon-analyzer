<script lang="ts">
  /**
   * The locality badge is the product's central promise, so it says something checkable
   * rather than something reassuring: open the network tab and there is no upload.
   *
   * The repository link is part of that same promise rather than decoration: the claim is
   * "read the parser before you trust it", and a claim like that needs somewhere to click.
   * The mark is an inline SVG — a remote icon font or CDN sprite would be a network request
   * on a page whose whole point is that it makes none, and the build's CSP would block it.
   */
  const REPO_URL = 'https://github.com/ggsurkov/csp-recon-analyzer';
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

    <!--
      Kept as one right-hand cluster so the two wrap together. Left loose, the link would
      wrap onto its own line at the first awkward width and read as a stray button.
    -->
    <div class="ml-auto flex items-center gap-4">
      <p class="max-w-md text-xs leading-relaxed text-slate-500">
        Your file is read by WebAssembly inside this tab. There is no upload endpoint in this
        app &mdash; open your browser's network panel and confirm it.
      </p>

      <a
        href={REPO_URL}
        target="_blank"
        rel="noopener noreferrer"
        aria-label="GitHub Repository"
        class="group flex shrink-0 items-center gap-2 rounded-lg border border-slate-700 px-3 py-2 text-xs font-medium text-slate-300 transition-colors hover:border-slate-500 hover:bg-slate-800/60 hover:text-slate-100 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-emerald-400"
      >
        <!-- The GitHub mark, 16px octicon geometry. `currentColor` so it inherits the hover. -->
        <svg
          viewBox="0 0 16 16"
          class="h-4 w-4 fill-current transition-colors group-hover:text-slate-100"
          aria-hidden="true"
          focusable="false"
        >
          <path
            d="M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38 0-.19-.01-.82-.01-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82a7.63 7.63 0 0 1 2-.27c.68 0 1.36.09 2 .27 1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8.01 8.01 0 0 0 16 8c0-4.42-3.58-8-8-8Z"
          />
        </svg>
        <!--
          Hidden on the narrowest screens, where the header is already three stacked rows and
          the mark alone is unambiguous. The link keeps its name either way: `aria-label` is
          on the anchor, not on this span.
        -->
        <span class="hidden sm:inline">GitHub</span>
      </a>
    </div>
  </div>
</header>
