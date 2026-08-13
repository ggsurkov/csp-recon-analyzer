<script lang="ts">
  import type { Finding } from '../types';
  import { VirtualWindow } from '../virtual-window.svelte';

  interface Props {
    findings: Finding[];
    thresholdDisplay: string;
  }

  let { findings, thresholdDisplay }: Props = $props();

  /**
   * Height of one rendered row, in CSS pixels. `h-[88px]` on the `<tr>` below must agree
   * with this number or the window drifts out of step with the scrollbar.
   *
   * 88px is what the tallest cell needs: a badge (20px) over two clamped lines of prose
   * (36px) inside `py-3` (24px). Every cell that could grow past that is clamped, which is
   * the price of admission for constant-height virtualisation — see VirtualWindow.
   */
  const ROW_HEIGHT = 88;

  /** Rows drawn beyond each edge so a fast flick never shows blank space. */
  const OVERSCAN = 5;

  const virtual = new VirtualWindow(ROW_HEIGHT, OVERSCAN);

  let scroller: HTMLDivElement;
  let head: HTMLTableSectionElement;

  /**
   * Measured rather than assumed, because the viewport height below is built from it and a
   * wrong constant shows up as a permanent sliver of scrollbar on a list short enough to
   * fit.
   */
  let headerHeight = $state(44);

  $effect(() => {
    const resize = new ResizeObserver(() => (headerHeight = head.offsetHeight));
    resize.observe(head);
    headerHeight = head.offsetHeight;
    return () => resize.disconnect();
  });

  $effect(() => virtual.observe(scroller));

  $effect(() => {
    virtual.count = findings.length;
  });

  // A new analysis is a new array; without this the next file opens halfway down the
  // previous one's scroll position, showing rows that are not there any more.
  $effect(() => {
    findings;
    scroller.scrollTop = 0;
    virtual.scrollTop = 0;
  });

  const visible = $derived(findings.slice(virtual.start, virtual.end));

  /**
   * Viewport height: tall enough to be worth scrolling, short enough to leave the summary
   * cards on screen. Capped at the exact height of the full list, so four findings render
   * as four rows rather than four rows and a void.
   *
   * This is an explicit `height`, not a `max-height`, and that is the whole trick. Left to
   * size itself from its content the container is circular — it is as tall as the rows
   * inside it, and the number of rows inside it is decided by how tall it is. On first
   * paint the content is just the header, so the container measures ~44px, asks for one row
   * and stays there. Computing the height from the row count instead breaks the cycle: it
   * is known before a single row exists.
   */
  const viewportStyle = $derived(
    `height: min(70vh, ${findings.length * ROW_HEIGHT + headerHeight}px)`
  );

  /** Long GUIDs would blow the column out; the full value stays in the tooltip. */
  function shortId(id: string): string {
    return id.length > 13 ? `${id.slice(0, 8)}…${id.slice(-4)}` : id;
  }

  function confidenceTone(finding: Finding): string {
    if (finding.evidence_kind === 'declared_and_priced') return 'bg-emerald-500/15 text-emerald-300';
    if (finding.evidence_kind === 'declared') return 'bg-sky-500/15 text-sky-300';
    return 'bg-amber-500/15 text-amber-300';
  }

  function windowTone(finding: Finding): string {
    return finding.remediation_window === 'NOW'
      ? 'bg-emerald-500/15 text-emerald-300'
      : 'bg-slate-700/50 text-slate-300';
  }
</script>

<div class="overflow-hidden rounded-2xl border border-slate-800">
  <!--
    One element scrolls in both directions: vertically past the findings, horizontally on a
    narrow screen. `sticky` inside it pins the header to the top of this box rather than to
    the page, which is what keeps the column labels attached to the rows they describe.
  -->
  <div bind:this={scroller} class="overflow-auto" style={viewportStyle}>
    <!--
      `table-fixed` plus an explicit colgroup is load-bearing, not cosmetic. With automatic
      layout the browser sizes columns from the rows currently in the DOM — and under
      virtualisation that set changes on every scroll frame, so the columns would visibly
      twitch as rows swapped in and out. Fixed widths make the layout independent of which
      window happens to be rendered.
    -->
    <table
      class="w-full min-w-[68rem] table-fixed border-collapse text-left text-sm"
      aria-rowcount={findings.length}
    >
      <!--
        Fixed widths mean a column can no longer grow to fit its content, so "Per month"
        gets the room its widest case actually needs: the "under the … gate" tag on a
        suppressed row. Sized against the 68rem minimum, not the comfortable case — below
        that width the table scrolls horizontally rather than clipping.
      -->
      <colgroup>
        <col style="width: 12%" />
        <col style="width: 12%" />
        <col style="width: 13%" />
        <col style="width: 11%" />
        <col style="width: 9%" />
        <col style="width: 15%" />
        <col style="width: 14%" />
        <col style="width: 14%" />
      </colgroup>

      <thead
        bind:this={head}
        class="sticky top-0 z-10 bg-slate-900 text-xs uppercase tracking-wider text-slate-400"
      >
        <tr>
          <th scope="col" class="px-4 py-3 font-medium">Customer</th>
          <th scope="col" class="px-4 py-3 font-medium">Subscription</th>
          <th scope="col" class="px-4 py-3 font-medium">SKU</th>
          <th scope="col" class="px-4 py-3 text-right font-medium">Effective price</th>
          <th scope="col" class="px-4 py-3 text-right font-medium">EST fee</th>
          <th scope="col" class="px-4 py-3 text-right font-medium">Per month</th>
          <th scope="col" class="px-4 py-3 font-medium">Confidence</th>
          <th scope="col" class="px-4 py-3 font-medium">Action</th>
        </tr>
      </thead>

      <tbody class="divide-y divide-slate-800">
        <!--
          The two spacers carry the height of every row that is not rendered, so the
          scrollbar reflects all 37,000 findings while the DOM holds ~40 of them.
        -->
        {#if virtual.padTop > 0}
          <tr aria-hidden="true" style="height: {virtual.padTop}px"></tr>
        {/if}

        {#each visible as finding, i (finding.record_index)}
          <!--
            Every row renders at full opacity, including sub-threshold ones. Dimming was
            doing two jobs badly: as a status signal it is invisible to anyone who cannot
            perceive the contrast step, and at 45% the row read as a rendering fault rather
            than as a deliberate state. The gate tag in the "Per month" column says it in
            words instead.

            `aria-rowindex` is what tells a screen reader it is on row 12,431 of 37,023 —
            without it the windowed table would announce eight rows and nothing else.
          -->
          <tr
            class="h-[88px] align-top hover:bg-slate-900/40"
            aria-rowindex={virtual.start + i + 1}
          >
            <td class="overflow-hidden px-4 py-3">
              <div class="truncate font-medium text-slate-100" title={finding.customer_name}>
                {finding.customer_name || '—'}
              </div>
              <div class="text-xs text-slate-500">row {finding.record_index}</div>
            </td>

            <td class="overflow-hidden px-4 py-3">
              <span class="font-mono text-xs text-slate-400" title={finding.subscription_id}>
                {shortId(finding.subscription_id)}
              </span>
              {#if finding.charge_start_date}
                <div class="truncate text-xs text-slate-600">
                  {finding.charge_start_date} → {finding.charge_end_date}
                </div>
              {/if}
            </td>

            <td class="overflow-hidden px-4 py-3 text-slate-300">
              <div class="line-clamp-2 leading-snug" title={finding.sku_name}>
                {finding.sku_name || finding.product_id}
              </div>
            </td>

            <td class="tnum overflow-hidden px-4 py-3 text-right">
              <div class="truncate text-slate-100">{finding.effective_unit_price.display}</div>
              <div class="truncate text-xs text-slate-500">
                list {finding.base_unit_price.display} × {finding.billable_quantity}
              </div>
            </td>

            <td class="tnum overflow-hidden px-4 py-3 text-right">
              <span
                class="inline-block rounded bg-amber-500/15 px-2 py-0.5 text-xs font-semibold text-amber-300"
              >
                {finding.rate_label}
              </span>
              <div class="mt-1 truncate text-xs text-slate-500">
                {finding.uplift_per_seat.display}/seat
              </div>
            </td>

            <td class="tnum overflow-hidden px-4 py-3 text-right font-semibold text-amber-300">
              <div class="truncate">{finding.monthly_run_rate.display}</div>
              {#if finding.suppressed}
                <div class="mt-1">
                  <span
                    class="inline-block whitespace-nowrap rounded bg-slate-700/50 px-1.5 py-0.5 text-[11px] font-medium text-slate-300"
                  >
                    under the {thresholdDisplay} gate
                  </span>
                </div>
              {/if}
            </td>

            <td class="overflow-hidden px-4 py-3">
              <span class="rounded px-2 py-0.5 text-xs font-medium {confidenceTone(finding)}">
                {finding.confidence}
              </span>
              <div
                class="mt-1 line-clamp-2 text-xs leading-snug text-slate-500"
                title={finding.confidence_label}
              >
                {finding.confidence_label}
              </div>
            </td>

            <td class="overflow-hidden px-4 py-3">
              <span class="rounded px-2 py-0.5 text-xs font-semibold {windowTone(finding)}">
                {finding.remediation_window.replace('_', ' ')}
              </span>
              <div
                class="mt-1 line-clamp-2 text-xs leading-snug text-slate-400"
                title={finding.action}
              >
                {finding.action}
              </div>
            </td>
          </tr>
        {/each}

        {#if virtual.padBottom > 0}
          <tr aria-hidden="true" style="height: {virtual.padBottom}px"></tr>
        {/if}
      </tbody>
    </table>
  </div>

  {#if findings.length > visible.length}
    <div
      class="border-t border-slate-800 bg-slate-900/60 px-4 py-2 text-xs text-slate-500"
      aria-live="off"
    >
      Scroll for all <span class="tnum text-slate-300">{findings.length.toLocaleString()}</span>
      lines — rows are drawn as they come into view, so the list stays responsive at any size.
    </div>
  {/if}
</div>
