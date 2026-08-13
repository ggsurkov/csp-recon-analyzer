<script lang="ts">
  import { fade, fly } from 'svelte/transition';
  import { analyzeBytes, analyzeFile, warmUpAnalyzer } from './lib/analyzer';
  import { DEMO_FILE_NAME, readDemoFile } from './lib/demo-file';
  import Dropzone from './lib/components/Dropzone.svelte';
  import EstExplainer from './lib/components/EstExplainer.svelte';
  import FindingsTable from './lib/components/FindingsTable.svelte';
  import Header from './lib/components/Header.svelte';
  import ProgressCard from './lib/components/ProgressCard.svelte';
  import StatCard from './lib/components/StatCard.svelte';
  import type { AnalysisProgress, AnalysisResult } from './lib/types';

  let result = $state<AnalysisResult | null>(null);
  let error = $state<string | null>(null);
  let busy = $state(false);
  let fileName = $state('');
  let progress = $state<AnalysisProgress | null>(null);

  /**
   * The demo file analyses in single-digit milliseconds. Showing the progress card for that
   * long is a flash of layout, which looks like a glitch rather than feedback — so the card
   * only appears once the work has proved it will take long enough to be worth watching.
   */
  const PROGRESS_REVEAL_DELAY_MS = 150;
  let showProgress = $state(false);
  let revealTimer: ReturnType<typeof setTimeout> | undefined;

  /**
   * Compile the analyser as the page loads, not when a file lands on the drop zone.
   *
   * This is what makes the offline claim true rather than nearly true. The worker's two
   * module fetches happen now, alongside the page's own assets, and the compiled module
   * then lives in worker memory for the session — so a drop with the network disconnected
   * runs exactly like a drop with it connected. Started here, unawaited: the rest of the
   * page does not depend on it.
   */
  let analyzerReady = $state(false);
  let analyzerError = $state<string | null>(null);

  warmUpAnalyzer().then(
    () => (analyzerReady = true),
    (e: unknown) => (analyzerError = e instanceof Error ? e.message : String(e))
  );

  const hasFindings = $derived((result?.findings.length ?? 0) > 0);

  /** "3.9 ms" reads as precision; "0.004 ms" reads as a bug. */
  const timing = $derived.by(() => {
    const ms = result?.execution_time_ms ?? 0;
    return ms < 1 ? `${ms.toFixed(2)} ms` : `${Math.round(ms)} ms`;
  });

  /** Sentences assembled here rather than in the markup, where nesting them hid a stray space. */
  const leakHint = $derived.by(() => {
    if (!result) return '';
    if (!hasFindings) return 'No Extended Service Terms uplift found in this file.';

    const parts = [`Per month, across ${result.reportable_subscription_count} subscription(s).`];
    if (result.suppressed_subscription_count > 0) {
      parts.push(
        `A further ${result.suppressed_monthly.display}/mo across ` +
          `${result.suppressed_subscription_count} subscription(s) is below the ` +
          `${result.noise_threshold_monthly.display} noise gate — listed below, ` +
          `excluded from this figure.`
      );
    }
    return parts.join(' ');
  });

  const rowsHint = $derived.by(() => {
    if (!result) return '';
    const parts = ['Parsed in WebAssembly inside this tab.'];
    if (result.rows_failed > 0) parts.push(`${result.rows_failed} row(s) were unreadable.`);
    if (result.row_error_count > 0) {
      parts.push(`${result.row_error_count} cell(s) failed to parse.`);
    }
    return parts.join(' ');
  });

  async function run(name: string, work: (report: (p: AnalysisProgress) => void) => Promise<AnalysisResult>) {
    busy = true;
    error = null;
    result = null;
    progress = null;
    fileName = name;

    clearTimeout(revealTimer);
    showProgress = false;
    revealTimer = setTimeout(() => (showProgress = true), PROGRESS_REVEAL_DELAY_MS);

    try {
      // Assigning the whole object once per tick is the entire update path: `$state` is
      // deep-reactive, the card reads it through `$derived`, and Svelte batches the DOM
      // write into a microtask. Nothing here loops or retains — the previous tick is
      // garbage the moment it is replaced.
      result = await work((p) => (progress = p));
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      clearTimeout(revealTimer);
      showProgress = false;
      busy = false;
      progress = null;
    }
  }

  function onFile(file: File) {
    void run(file.name, (report) => analyzeFile(file, report));
  }

  function onDemo() {
    // The bytes are compiled into the bundle, so this reads them out of memory and hands
    // them straight to the worker. No request, no server, nothing that can be offline —
    // see lib/demo-file.ts.
    void run(DEMO_FILE_NAME, (report) => analyzeBytes(readDemoFile(), report));
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

        {#if showProgress}
          <div in:fade={{ duration: 150 }}>
            <ProgressCard {progress} {fileName} />
          </div>
        {:else}
          <Dropzone onfile={onFile} ondemo={onDemo} {busy} ready={analyzerReady} initError={analyzerError} />
        {/if}

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
      <!--
        The results replace a bar that just reached 100%, so they rise into place rather
        than appearing instantly — it reads as the same object continuing, not a new screen.
      -->
      <section class="space-y-8" in:fly={{ y: 8, duration: 250, delay: 60 }}>
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
            hint={leakHint}
          />
          <StatCard
            label="Rows processed"
            value={`${result.rows_parsed.toLocaleString()} rows / ${timing}`}
            hint={rowsHint}
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

    <!--
      Shown on both views deliberately. Before an analysis it is the explanation of why
      anyone should bother; after one it is the citation behind the number on screen.
    -->
    <EstExplainer />
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
      <p>
        MIT licensed. The parser core is open — read it before you trust it.
        {#if result}
          <span class="text-slate-600">Analyser build {result.analyzer_version}.</span>
        {/if}
      </p>
    </div>
  </footer>
</div>
