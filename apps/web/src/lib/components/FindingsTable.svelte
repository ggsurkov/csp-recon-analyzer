<script lang="ts">
  import type { Finding } from '../types';

  interface Props {
    findings: Finding[];
    thresholdDisplay: string;
  }

  let { findings, thresholdDisplay }: Props = $props();

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
  <div class="overflow-x-auto">
    <table class="w-full min-w-[68rem] text-left text-sm">
      <thead class="bg-slate-900/80 text-xs uppercase tracking-wider text-slate-400">
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
        {#each findings as finding (finding.record_index)}
          <tr class="align-top {finding.suppressed ? 'opacity-45' : ''} hover:bg-slate-900/40">
            <td class="px-4 py-3">
              <div class="font-medium text-slate-100">{finding.customer_name || '—'}</div>
              <div class="text-xs text-slate-500">row {finding.record_index}</div>
            </td>

            <td class="px-4 py-3">
              <span class="font-mono text-xs text-slate-400" title={finding.subscription_id}>
                {shortId(finding.subscription_id)}
              </span>
              {#if finding.charge_start_date}
                <div class="text-xs text-slate-600">
                  {finding.charge_start_date} → {finding.charge_end_date}
                </div>
              {/if}
            </td>

            <td class="px-4 py-3 text-slate-300">{finding.sku_name || finding.product_id}</td>

            <td class="tnum px-4 py-3 text-right">
              <div class="text-slate-100">{finding.effective_unit_price.display}</div>
              <div class="text-xs text-slate-500">
                list {finding.base_unit_price.display} × {finding.billable_quantity}
              </div>
            </td>

            <td class="tnum px-4 py-3 text-right">
              <span
                class="inline-block rounded bg-amber-500/15 px-2 py-0.5 text-xs font-semibold text-amber-300"
              >
                {finding.rate_label}
              </span>
              <div class="mt-1 text-xs text-slate-500">
                {finding.uplift_per_seat.display}/seat
              </div>
            </td>

            <td class="tnum px-4 py-3 text-right font-semibold text-amber-300">
              {finding.monthly_run_rate.display}
              {#if finding.suppressed}
                <div class="text-xs font-normal text-slate-500">
                  under the {thresholdDisplay} gate
                </div>
              {/if}
            </td>

            <td class="px-4 py-3">
              <span class="rounded px-2 py-0.5 text-xs font-medium {confidenceTone(finding)}">
                {finding.confidence}
              </span>
              <div class="mt-1 max-w-[16rem] text-xs leading-snug text-slate-500">
                {finding.confidence_label}
              </div>
            </td>

            <td class="px-4 py-3">
              <span class="rounded px-2 py-0.5 text-xs font-semibold {windowTone(finding)}">
                {finding.remediation_window.replace('_', ' ')}
              </span>
              <div class="mt-1 max-w-[20rem] text-xs leading-snug text-slate-400">
                {finding.action}
              </div>
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  </div>
</div>
