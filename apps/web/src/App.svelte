<script lang="ts">
  import { analyzeBytes, analyzeFile } from './lib/analyzer';
  import Dropzone from './lib/components/Dropzone.svelte';
  import FindingsTable from './lib/components/FindingsTable.svelte';
  import Header from './lib/components/Header.svelte';
  import StatCard from './lib/components/StatCard.svelte';
  import type { AnalysisResult } from './lib/types';

  let result = $state<AnalysisResult | null>(null);
  let error = $state<string | null>(null);
  let busy = $state(false);
  let fileName = $state('');

  const hasFindings = $derived((result?.findings.length ?? 0) > 0);

  /** "3.9 ms" reads as precision; "0.004 ms" reads as a bug. */
  const timing = $derived.by(() => {
    const ms = result?.execution_time_ms ?? 0;
    return ms < 1 ? `${ms.toFixed(2)} ms` : `${Math.round(ms)} ms`;
  });

  async function run(name: string, work: () => Promise<AnalysisResult>) {
    busy = true;
    error = null;
    result = null;
    fileName = name;
    try {
      result = await work();
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      busy = false;
    }
  }

  function onFile(file: File) {
    void run(file.name, () => analyzeFile(file));
  }

  function onDemo() {
    void run('mock_recon_2026.csv.gz', async () => {
      // Same-origin static asset shipped with the app. Nothing is sent anywhere.
      //
      // The `.bin` extension is load-bearing: served as `.csv.gz`, static hosts set
      // `Content-Encoding: gzip`, the browser inflates the body before `fetch` sees it,
      // and its decoder stops at the first gzip member — silently delivering half the
      // rows. See scripts/copy-fixture.mjs.
      const response = await fetch('demo/mock_recon_2026.csv.gz.bin');
      if (!response.ok) {
        throw new Error(
          `Could not load the demo file (HTTP ${response.status}). Run \`npm run demo\` to copy it into public/.`
        );
      }
      return analyzeBytes(await response.arrayBuffer());
    });
  }

  function reset() {
    result = null;
    error = null;
    fileName = '';
  }
</script>

<div class="min-h-full">
  <Header />

  <main class="mx-auto max-w-7xl space-y-8 px-6 py-10">
    {#if !result}
      <section class="space-y-6">
        <div class="max-w-3xl space-y-3">
          <h1 class="text-3xl font-semibold tracking-tight text-slate-50">
            Find the Extended Service Terms penalties in your Microsoft invoice
          </h1>
          <p class="leading-relaxed text-slate-400">
            Since <span class="text-slate-200">4 May 2026</span> a CSP subscription that is neither
            renewed nor cancelled no longer gets a free grace period. It rolls onto a monthly term
            at <span class="font-semibold text-amber-300">+3%</span>, or
            <span class="font-semibold text-amber-300">+23%</span> when the SKU has no monthly plan.
            The line looks identical to last month's. Only the price moved.
          </p>
        </div>

        <Dropzone onfile={onFile} ondemo={onDemo} {busy} />

        {#if error}
          <div
            class="rounded-xl border border-red-500/30 bg-red-500/10 px-5 py-4 text-sm text-red-200"
            role="alert"
          >
            <div class="font-semibold">That file could not be analysed</div>
            <p class="mt-1 text-red-300/90">{error}</p>
          </div>
        {/if}
      </section>
    {:else}
      <section class="space-y-8">
        <div class="flex flex-wrap items-baseline justify-between gap-4">
          <div>
            <h1 class="text-2xl font-semibold tracking-tight text-slate-50">Analysis</h1>
            <p class="mt-1 font-mono text-sm text-slate-500">{fileName}</p>
          </div>
          <button
            type="button"
            class="rounded-lg border border-slate-700 px-4 py-2 text-sm font-medium text-slate-300 transition hover:border-slate-500 hover:text-slate-100"
            onclick={reset}
          >
            Analyse another file
          </button>
        </div>

        <div class="grid gap-4 md:grid-cols-2">
          <StatCard
            label="EST penalty leak"
            value={result.total_est_leak_monthly.display}
            accent="amber"
            hint={hasFindings
              ? `Per month, across ${result.reportable_subscription_count} subscription(s). ${
                  result.suppressed_subscription_count > 0
                    ? `A further ${result.suppressed_monthly.display}/mo across ${result.suppressed_subscription_count} subscription(s) is below the ${result.noise_threshold_monthly.display} noise gate — listed below, excluded from this figure.`
                    : ''
                }`
              : 'No Extended Service Terms uplift found in this file.'}
          />
          <StatCard
            label="Rows processed"
            value={`${result.rows_parsed.toLocaleString()} rows / ${timing}`}
            hint={`Parsed in WebAssembly inside this tab.${
              result.rows_failed > 0 ? ` ${result.rows_failed} row(s) were unreadable.` : ''
            }${result.row_error_count > 0 ? ` ${result.row_error_count} cell(s) failed to parse.` : ''}`}
          />
        </div>

        {#if result.warnings.length > 0}
          <div class="space-y-2">
            {#each result.warnings as warning (warning)}
              <div
                class="rounded-xl border border-amber-500/25 bg-amber-500/5 px-5 py-3 text-sm text-amber-200/90"
              >
                {warning}
              </div>
            {/each}
          </div>
        {/if}

        {#if hasFindings}
          <div class="space-y-3">
            <div class="flex flex-wrap items-baseline justify-between gap-3">
              <h2 class="text-lg font-semibold text-slate-100">
                Findings <span class="text-slate-500">({result.findings.length})</span>
              </h2>
              <p class="text-xs text-slate-500">
                Every row points at a line in your file. Nothing here is inferred by a model.
              </p>
            </div>
            <FindingsTable
              findings={result.findings}
              thresholdDisplay={result.noise_threshold_monthly.display}
            />
          </div>
        {:else}
          <div
            class="rounded-2xl border border-slate-800 bg-slate-900/40 px-6 py-10 text-center text-sm text-slate-400"
          >
            <div class="mb-2 text-2xl" aria-hidden="true">✅</div>
            <p class="font-medium text-slate-200">No EST uplift in this export.</p>
            <p class="mt-1">
              That is a real result, not a failure to look — {result.rows_parsed.toLocaleString()} rows
              were checked against the +3% and +23% rates in force from 4 May 2026.
            </p>
          </div>
        {/if}
      </section>
    {/if}
  </main>

  <footer class="border-t border-slate-800 px-6 py-8">
    <div class="mx-auto max-w-7xl space-y-2 text-xs leading-relaxed text-slate-500">
      <p>
        <span class="text-slate-400">What this checks:</span> Extended Service Terms uplift only.
        Duplicate subscriptions, expired promotions, dormant auto-renewals and proration anomalies
        are not implemented yet.
      </p>
      <p>
        <span class="text-slate-400">Confidence:</span> a finding at 0.75 was inferred from the
        price ratio alone because Microsoft did not label the line. Verify before you act on it.
      </p>
      <p>MIT licensed. The parser core is open — read it before you trust it.</p>
    </div>
  </footer>
</div>
