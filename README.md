# csp-recon-analyzer

Parser and deterministic leak detectors for **Microsoft Partner Center license-based
reconciliation exports** (direct CSP, NCE, 2026 schema).

Your recon file is tens of thousands of lines of 18-decimal prices and proration
fragments. This reads it and tells you which of those lines are money you are paying for
nothing — and, for each one, whether you can act on it today or only at renewal.

MIT licensed. No network calls anywhere in the parser.

---

## Status

Early. `recon-core` parses the export and implements one detector.

| | |
|---|---|
| ✅ | Streaming `.csv.gz` reader (multi-member gzip, BOM, Excel guards, schema drift) |
| ✅ | `EST_UPLIFT` — Extended Service Terms penalty detection (+3% / +23%) |
| ✅ | WASM bindings + browser UI (Svelte 5, drag-and-drop, nothing uploaded) |
| ⬜ | `DUPLICATE_SUBSCRIPTION`, `PROMO_EXPIRED`, `DORMANT_AUTORENEW`, `PRORATION_ANOMALY` |

## Quick start

```bash
cargo test                                  # 56 tests, runs against the committed fixture
python scripts/generate_mock_recon.py       # regenerate fixtures/mock_recon_2026.csv.gz

cd apps/web && npm install && npm run dev   # builds the WASM, then serves the UI
```

```rust
use recon_core::{stream_recon_gz, ParseOptions, detectors::est::EstDetector};

let file = std::fs::File::open("recon.csv.gz")?;
let mut est = EstDetector::new();
let stats = stream_recon_gz(file, &ParseOptions::default(), |row| est.observe(row))?;

let report = est.finish();
for sub in report.reportable() {
    println!(
        "{:>10} {}/mo  {}  {}  [{}]",
        sub.monthly_run_rate, sub.currency, sub.customer_name, sub.sku_name,
        sub.remediation_window.as_str(),
    );
}
```

## Why Extended Service Terms

Until **4 May 2026** a CSP subscription that was neither renewed nor cancelled sat in a
free 30-day grace period. Microsoft removed that. The same subscription now rolls onto an
**Extended Service Term** — a monthly term charged at **+3%** over list, or **+23%** when
the SKU has no monthly plan.

Nothing about the line looks different. Same SKU, same seat count, same customer. The only
signal is the price, and it is one column away from a column that still shows list price.
It is a calendar-driven leak that accrues quietly and forever.

## What the parser handles that a spreadsheet does not

| Reality | What breaks without it |
|---|---|
| **Multi-member gzip** — the export is a *set* of blobs | `GzDecoder` reads the first member and stops. No error. Just a smaller invoice. |
| **UTF-8 BOM** welded to `PartnerId` | The first column silently binds to nothing |
| **Excel-guard apostrophes** — `'22.000000000000000000` | Every price parses as text |
| **Scientific notation** — `'0E-20` in `TaxTotal` | `from_str` rejects it |
| **`ChargeType` casing** — `cycleCharge`/`cyclecharge`/`CycleCharge` in one file | Rows land in the wrong bucket |
| **`ReferenceId`** — scalar before June 2026, JSON object after | Half the file fails to key |
| **Negative `BillableQuantity`** on credits | Multiplying by a charge-type sign flips credits into charges |
| **Schema drift** — Microsoft adds columns | A strict reader rejects the whole file |

## Rules the code is held to

1. **Money is `rust_decimal::Decimal`, never `f64`.** We make claims to the cent about
   somebody's real invoice.
2. **Cost is `EffectiveUnitPrice × BillableQuantity`.** `UnitPrice` is list, not what was
   charged. `Quantity` is the subscription size, not what the line billed for. Using either
   produces numbers that look plausible and are wrong on every prorated line.
3. **Every finding carries a remediation window** — `NOW` / `AT_RENEWAL` / `NEVER`.
   Recommending a seat reduction that is contractually impossible is a P0 bug.
4. **Policy constants live in dated tables** (`EstPolicy::table`), never as literals in a
   branch. Reprocessing a February file must apply February's rules.
5. **Suppress, never delete.** Findings under the $5/month noise gate stay in the report
   and out of the headline, with the suppressed total shown as a footnote.
6. **Every finding points at the rows it came from.** No inference, no LLM, no "probably".

## What this is not

- Not a billing system, and not a Partner Center replacement.
- It does not write anything back to your PSA.
- It does not talk to Graph, Partner Center, or anything else. It reads bytes you already
  have.
- Nothing here is medical-grade certainty: ratio-only EST detections are reported at
  `confidence 0.75` and say so.

## The web app

`apps/web` is a Svelte 5 SPA. Drop a file in, get the findings table. The parsing happens
in a Web Worker running the WASM build of `recon-core` — the file is read from disk into
the tab and never goes anywhere else.

Two things back that claim up rather than merely stating it:

- **No networking exists in the analysis path.** `recon-wasm` depends on `csv`, `flate2`,
  `chrono`, `rust_decimal`, `serde`, `js-sys` and `wasm-bindgen`. None of them opens a
  socket. There is no upload endpoint to disable.
- **The production build ships a CSP with `connect-src 'self'`**, so even a compromised
  dependency has nowhere to send anything. Injected at build time from `vite.config.ts`.

### ⚠️ Never serve `.csv.gz` as a static asset

Static hosts — Vite preview, nginx, S3, Netlify, GitHub Pages — see a `.gz` extension and
answer with `Content-Encoding: gzip`. The browser then inflates the body before `fetch`
sees it, and **its decoder stops after the first gzip member**. A Partner Center export is
multi-member, so the app receives the first blob only: in our fixture, 5,349 bytes of an
11,246-byte file, half the rows missing, no error anywhere.

That is the same data-loss bug `MultiGzDecoder` exists to prevent, reintroduced one layer
below the parser and invisible from inside it. The demo asset is therefore published as
`mock_recon_2026.csv.gz.bin`. Anything you host, host opaque.

Files a user drags in are unaffected — `File.arrayBuffer()` returns the bytes off disk.

## Repository layout

```
crates/recon-core/          the parser and detectors
  src/numeric.rs            Excel guards, scientific notation, dates
  src/types.rs              ReconRow, ChargeType, ReferenceId, BillingTerm
  src/parser.rs             streaming reader, column mapping, drift tolerance
  src/detectors/est.rs      EST_UPLIFT
  tests/parser_test.rs      end-to-end against the fixture
crates/recon-wasm/          browser bindings
  src/report.rs             the JSON contract with the UI — host-testable, no wasm types
  src/lib.rs                parse_recon_bytes(), timing, panic hook
apps/web/                   Svelte 5 + Vite + Tailwind SPA
  src/lib/recon.worker.ts   runs the WASM off the main thread
  src/lib/analyzer.ts       main-thread client
scripts/generate_mock_recon.py   fixture generator (deterministic, synthetic)
fixtures/mock_recon_2026.csv.gz  14 rows, every edge case above
```

## Fixtures

`fixtures/` is entirely synthetic. Real reconciliation exports contain partner and customer
tenant identifiers and purchase prices, and `.gitignore` is set up to keep them out of the
repository. If you want to check a real file, run the tool locally — it never uploads
anything.

## License

MIT. See [LICENSE](LICENSE).
