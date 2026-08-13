<script lang="ts">
  /**
   * The "why should I believe you" panel.
   *
   * A tool that tells a partner they are being overcharged has to show its working, and the
   * working here is Microsoft's own documentation. Every claim below is one click from the
   * page that states it — no paraphrase we could have got wrong, no blog post that might be
   * out of date.
   *
   * Native `<details>` rather than a `$state` toggle: it is keyboard-operable, findable by
   * in-page search even while collapsed, and needs no JavaScript to work.
   */

  interface Props {
    /** Open on first paint. Collapsed by default so it never buries the dropzone. */
    open?: boolean;
  }

  let { open = false }: Props = $props();

  /**
   * Microsoft Learn only. Vendor blogs summarising this change disagree with each other on
   * the dates, which is precisely why none of them are cited here.
   */
  const docs = [
    {
      href: 'https://learn.microsoft.com/en-us/partner-center/customers/extended-service-terms',
      title: 'Use Extended Service Terms (EST) for CSP subscriptions',
      note: 'The primary reference. States the monthly-list repricing, the +3% surcharge and the three end-of-term options.'
    },
    {
      href: 'https://learn.microsoft.com/en-us/partner-center/customers/extended-service-terms#billing-and-recon-files-for-est',
      title: 'Billing and recon files for EST',
      note: 'How EST charges appear in your reconciliation file — this is the section this tool implements.'
    },
    {
      href: 'https://learn.microsoft.com/en-us/partner-center/customers/extended-service-terms#est-backfill',
      title: 'EST backfill',
      note: 'Why you may be paying for an EST nobody on your team ever selected.'
    },
    {
      href: 'https://learn.microsoft.com/en-us/partner-center/announcements/2026-february#revised-timelines-extended-service-terms-in-csp',
      title: 'Revised timelines: Extended Service Terms in CSP',
      note: 'Partner Center announcement, 5 February 2026. Moved enforcement from 1 April to 4 May 2026.'
    },
    {
      href: 'https://aka.ms/ExtendedServiceTerms-FAQ',
      title: 'Extended Service Terms — partner FAQ',
      note: 'Microsoft’s own Q&A, published alongside the October 2025 partner notification.'
    }
  ];
</script>

<details class="group rounded-2xl border border-slate-800 bg-slate-900/40" {open}>
  <summary
    class="flex cursor-pointer items-center gap-3 rounded-2xl px-6 py-4 text-left font-medium text-slate-200 transition hover:bg-slate-900/60 focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-emerald-500"
  >
    <span
      class="text-slate-500 transition-transform group-open:rotate-90 motion-reduce:transition-none"
      aria-hidden="true">▶</span
    >
    <span>Why EST matters &mdash; and the official Microsoft documentation</span>
    <span
      class="ml-auto hidden shrink-0 rounded-full bg-amber-500/10 px-2.5 py-1 text-xs font-semibold text-amber-300 sm:inline"
    >
      +3% surcharge
    </span>
  </summary>

  <div class="space-y-8 border-t border-slate-800 px-6 py-6 text-sm leading-relaxed text-slate-400">
    <section class="space-y-3">
      <h3 class="text-xs font-semibold uppercase tracking-wider text-slate-300">
        The problem
      </h3>
      <p>
        Until <span class="text-slate-200">4 May 2026</span>, a CSP subscription that reached its
        end date without being renewed dropped into a <em>free</em> 30-day grace period. Microsoft
        removed that. A subscription in the same state now rolls onto an
        <span class="text-slate-200">Extended Service Term</span>: it is repriced onto the standard
        <em>monthly</em> list price and charged a surcharge on top of that.
      </p>

      <div class="overflow-x-auto">
        <table class="w-full min-w-[26rem] text-left">
          <thead class="text-xs uppercase tracking-wider text-slate-500">
            <tr>
              <th scope="col" class="py-2 pr-4 font-medium">Figure</th>
              <th scope="col" class="py-2 font-medium">What it is</th>
            </tr>
          </thead>
          <tbody class="divide-y divide-slate-800">
            <tr>
              <td class="py-2 pr-4 align-top">
                <span
                  class="rounded bg-amber-500/15 px-2 py-0.5 text-xs font-semibold text-amber-300"
                  >+3%</span
                >
              </td>
              <td class="py-2 align-top">
                The EST surcharge itself, applied over monthly list price. This is the whole fee,
                and it is what this tool reports.
              </td>
            </tr>
            <tr>
              <td class="py-2 pr-4 align-top">
                <span
                  class="rounded bg-slate-700/50 px-2 py-0.5 text-xs font-semibold text-slate-300"
                  >~23%+</span
                >
              </td>
              <td class="py-2 align-top">
                The effective jump on your invoice when a subscription moves off an
                <em>annual</em> commitment — roughly 20% of it is the annual discount you no longer
                get, not a Microsoft fee. It is a comparison against last cycle, so it is not
                visible inside a single recon line.
              </td>
            </tr>
          </tbody>
        </table>
      </div>

      <p class="rounded-xl border border-amber-500/25 bg-amber-500/5 px-4 py-3 text-amber-200/90">
        <span class="font-semibold">You may not have chosen this.</span> Microsoft converted eligible
        subscriptions that had auto-renew off <em>without an explicit cancellation</em> into ESTs,
        with auto-renew set back on, so that customers would not lose service. Setting auto-renew to
        false through the API still does this today unless a <code
          class="rounded bg-slate-800 px-1 py-0.5 text-xs text-slate-300">scheduledActions</code
        > cancel instruction is sent with it.
      </p>

      <p>
        Nothing about the line looks different. The seat count does not move, the SKU name is the
        one you know, and the invoice grows by a few percent. Across 40,000 rows of prorated
        reconciliation data, nobody finds that by reading.
      </p>
    </section>

    <section class="space-y-3">
      <h3 class="text-xs font-semibold uppercase tracking-wider text-slate-300">
        What this tool does about it
      </h3>
      <p>
        Drop the license-based reconciliation export in and every EST charge is priced, totalled
        per subscription and expressed as dollars per month &mdash; in milliseconds, inside this
        tab. Microsoft documents two ways to recognise an EST line: the SKU name carries
        <span class="text-slate-300">&ldquo;Extended Service Term&rdquo;</span>, or the price sits
        at a known multiple of list. This build checks both, reports which one fired, and drops the
        confidence on any finding that rests on the price ratio alone.
      </p>
      <p>
        Every finding also says whether you can act <span class="text-slate-300">now</span> or only
        <span class="text-slate-300">at renewal</span>. An EST is itself a monthly term, and
        Microsoft allows cancelling one at any point in that term &mdash; which is what makes these
        findings worth acting on the day you read them.
      </p>
    </section>

    <section class="space-y-3">
      <h3 class="text-xs font-semibold uppercase tracking-wider text-slate-300">
        Check it against the source
      </h3>
      <ul class="space-y-3">
        {#each docs as doc (doc.href)}
          <li>
            <a
              href={doc.href}
              target="_blank"
              rel="noopener noreferrer"
              class="group/link font-medium text-emerald-400 underline decoration-emerald-500/30 underline-offset-4 transition hover:text-emerald-300 hover:decoration-emerald-400"
            >
              {doc.title}<span class="ml-1 text-xs" aria-hidden="true">↗</span>
              <span class="sr-only">(opens in a new tab)</span>
            </a>
            <p class="mt-0.5 text-xs text-slate-500">{doc.note}</p>
          </li>
        {/each}
      </ul>
      <p class="text-xs text-slate-600">
        Opening these is the only thing on this page that touches the network, and it happens in a
        new tab. Your reconciliation file is not involved.
      </p>
    </section>
  </div>
</details>
