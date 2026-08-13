#!/usr/bin/env python3
"""
Generate `mock_recon_1gb.csv` — a ~1 GB synthetic Partner Center license-based
reconciliation export, for stress-testing the WASM parser + Web Worker pipeline.

This is the *volume* fixture. `scripts/generate_mock_recon.py` stays the *semantic*
fixture (14 hand-written rows, byte-reproducible, committed). The column schema is
imported from that script on purpose: two generators disagreeing about the header
would be a silent way to test the parser against a schema nobody ships.

Memory profile
--------------
O(chunk_rows), not O(file). Rows are accumulated into a buffer of `--chunk-rows`
(default 50 000 ≈ 25 MB of text), serialised, encoded, written to disk, dropped.
Nothing is kept across chunks: subscription identity is *derived* from a counter,
never stored in a pool, so a 1 GB run costs the same RAM as a 1 MB run.

Row mix (per row, measured and reported at the end — not assumed)
-----------------------------------------------------------------
  ~95.0 %  normal traffic: cycle charges, mid-cycle add/removeQuantity prorations,
           mixed `ChargeType` casing, an occasional unknown charge type, blank cells
  ~ 3.0 %  EST +3 %  — SKU has a monthly plan, subscription rolled onto monthly term
  ~ 1.8 %  EST +23 % — SKU has no monthly plan, annual subscription dropped into EST
  ~ 0.2 %  sub-threshold EST: real uplift worth ~$0.04/mo, i.e. below the $5/mo
           noise gate. The detector must find these and the report must suppress them.

Format fidelity (same traps as the small fixture)
-------------------------------------------------
UTF-8 BOM welded to the first header cell · Excel-guard leading apostrophe on every
numeric cell · 18-decimal money · `'0E-20` scientific zero in TaxTotal · CRLF line
endings · customer names containing commas · `ReferenceId` in both the legacy scalar
and the post-June-2026 JSON shape (embedded quotes → CSV escaping) · negative
quantities and subtotals · a 49th unknown column.

All money is computed in integer micro-dollars, so the 18-decimal cells are exact
rather than float-rounded.

Usage:
    python scripts/generate_1gb_recon.py                 # ~1000 MiB next to the repo root
    python scripts/generate_1gb_recon.py --size-mb 50    # quick smoke run
    python scripts/generate_1gb_recon.py -o D:/tmp/x.csv --seed 7
"""

from __future__ import annotations

import argparse
import calendar
import csv
import io
import os
import random
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from generate_mock_recon import COLUMNS, DRIFT_COLUMN, KNOWN_COLUMNS  # noqa: E402

assert len(KNOWN_COLUMNS) == 48, f"expected 48 canonical columns, got {len(KNOWN_COLUMNS)}"
assert len(COLUMNS) == 49 and COLUMNS[-1] == DRIFT_COLUMN

MIB = 1024 * 1024

# --------------------------------------------------------------------------------------
# Row classes and their target share of the file
# --------------------------------------------------------------------------------------

NORMAL, EST_3, EST_23, EST_SUB = "normal", "est_3", "est_23", "est_sub"

CLASS_WEIGHTS = {
    NORMAL: 0.950,
    EST_3: 0.030,
    EST_23: 0.018,
    EST_SUB: 0.002,
}
CLASS_NAMES = list(CLASS_WEIGHTS)
CLASS_CUM: list[float] = []
_acc = 0.0
for _name in CLASS_NAMES:
    _acc += CLASS_WEIGHTS[_name]
    CLASS_CUM.append(_acc)

# EST uplift rates. Kept as integer percent so `base * pct // 100` stays exact.
EST_PCT = {EST_3: 103, EST_23: 123, EST_SUB: 103}

NOISE_GATE_MICRO = 5_000_000  # $5.00/mo — the §3.5 step-6 gate the fixture must straddle

# --------------------------------------------------------------------------------------
# Static-ish data
# --------------------------------------------------------------------------------------

PARTNER_ID = "a1b2c3d4-0000-4000-8000-000000000001"
PARTNER_NAME = "Northstar Managed Services"
CURRENCY = "USD"
FX_RATE = "'1.000000000000000000"
FX_DATE = "2026-07-01T00:00:00Z"
ZERO_18 = "'0.000000000000000000"
SCI_ZERO = "'0E-20"
UNIT_TYPE = "Licenses"
PRODUCT_CATEGORY = "license-based"
PUBLISHER = "Microsoft"

# Names deliberately include commas, an ampersand and a quote-free apostrophe so the
# writer is forced into real CSV quoting on a meaningful share of rows.
CUSTOMER_STEMS = [
    "Contoso Ltd", "Fabrikam, Inc.", "Northwind Traders", "Adventure Works",
    "Tailspin Toys, LLC", "Wingtip Toys", "Proseware, Inc.", "Litware Holdings",
    "Fourth Coffee", "Graphic Design Institute", "Wide World Importers",
    "Blue Yonder Airlines", "Trey Research", "Woodgrove Bank", "Alpine Ski House",
    "Lucerne Publishing", "Margie's Travel", "Coho Vineyard & Winery",
    "Relecloud Systems", "VanArsdel, Ltd.",
]
COUNTRIES = ["US", "CA", "GB", "DE", "AU", "NL"]

# name, ProductId, SkuId, AvailabilityId, base unit price (micro-$), has monthly plan
CATALOG = [
    ("Microsoft 365 Business Premium", "CFQ7TTC0LCHC", "0002", "DZH318Z0BQ4B", 22_000_000, True),
    ("Microsoft 365 Business Standard", "CFQ7TTC0LDPB", "0001", "DZH318Z0BQ4C", 12_500_000, True),
    ("Exchange Online (Plan 1)", "CFQ7TTC0LH16", "0001", "DZH318Z0BQ3Q", 4_000_000, True),
    ("Microsoft Teams Phone Standard", "CFQ7TTC0MSSN", "0001", "DZH318Z0BQ5C", 8_000_000, True),
    ("Power BI Pro", "CFQ7TTC0L3PB", "0002", "DZH318Z0BQ6F", 14_000_000, True),
    ("Microsoft Defender for Office 365 (Plan 1)", "CFQ7TTC0LH0X", "0001", "DZH318Z0BQ8J", 1_500_000, True),
    ("Microsoft 365 E3", "CFQ7TTC0LFLX", "0002", "DZH318Z0BQ9K", 36_000_000, True),
    # No monthly plan → these are the SKUs that take the +23 % EST hit.
    ("Project Plan 3", "CFQ7TTC0HDB1", "0002", "DZH318Z0BQ4P", 30_000_000, False),
    ("Visio Plan 2", "CFQ7TTC0HD33", "0002", "DZH318Z0BQ7H", 15_000_000, False),
    ("Dynamics 365 Business Central Essentials", "CFQ7TTC0LH3N", "0001", "DZH318Z0BQ2M", 70_000_000, False),
    ("Power Automate Premium", "CFQ7TTC0KXG6", "0002", "DZH318Z0BQ1L", 15_000_000, False),
]
CATALOG_MONTHLY = [p for p in CATALOG if p[5]]
CATALOG_ANNUAL_ONLY = [p for p in CATALOG if not p[5]]

# A cheap SKU whose 3 % uplift lands at $0.039/mo on a single seat: real, and under the gate.
SUB_THRESHOLD_SKU = ("Microsoft Entra ID P1", "CFQ7TTC0LFLS", "0001", "DZH318Z0BQ0A", 1_300_000, True)

TERM_ANNUAL_MONTHLY = "Annual term, Monthly billing"
TERM_MONTHLY = "Monthly term, Monthly billing"
TERM_ANNUAL_ANNUAL = "Annual term, Annual billing"
TERM_TRIENNIAL = "Triennial term, Monthly billing"

CHARGE_CYCLE_CASINGS = ["cycleCharge", "cyclecharge", "CycleCharge"]
UNKNOWN_CHARGE_TYPES = ["tierTransitionFee", "conversionCharge", "reinstateFee"]

# Charge periods: every month of 2026. Consecutive periods on one subscription are what
# make PROMO_EXPIRED / DORMANT_AUTORENEW detectable at all.
MONTHS = [(2026, m) for m in range(1, 13)]

# --------------------------------------------------------------------------------------
# Formatting helpers. Money is integer micro-dollars → exact 18-decimal cells.
# --------------------------------------------------------------------------------------


def money(micro: int) -> str:
    """Excel-guarded 18-decimal money cell, e.g. -28.387096000000000000 → '-28.387096…"""
    sign = "-" if micro < 0 else ""
    micro = abs(micro)
    return f"'{sign}{micro // 1_000_000}.{micro % 1_000_000:06d}000000000000"


def qty(units: int) -> str:
    """Excel-guarded 4-decimal quantity cell."""
    return f"'{units}.0000"


def day_str(year: int, month: int, day: int) -> str:
    return f"{year:04d}-{month:02d}-{day:02d}T00:00:00Z"


def month_span(year: int, month: int) -> tuple[str, str, int]:
    last = calendar.monthrange(year, month)[1]
    return day_str(year, month, 1), day_str(year, month, last), last


def guid(prefix: str, n: int) -> str:
    """Deterministic well-formed GUID from a counter — no pool to keep in memory."""
    h = f"{n:012x}"
    return f"{prefix}-{h[0:4]}-4{h[4:7]}-8{h[7:10]}-{h[10:12]}0000000000"


# --------------------------------------------------------------------------------------
# Row construction
# --------------------------------------------------------------------------------------


def build_row(
    *,
    customer: tuple[str, str, str, str],
    product: tuple[str, str, str, str, int, bool],
    subscription_id: str,
    order_id: str,
    invoice: str,
    charge_start: str,
    charge_end: str,
    term: str,
    effective_unit_price_micro: int | None,
    unit_price_micro: int | None,
    quantity_units: int,
    billable_units: int,
    subtotal_micro: int | None,
    charge_type: str,
    tax_total: str,
    price_adjustment: str,
    reference_id: str,
    promotion_id: str,
    sub_start: str,
    sub_end: str,
    drift: str,
) -> list[str]:
    cust_id, cust_name, cust_domain, cust_country = customer
    sku_name, product_id, sku_id, availability_id, _base, _monthly = product

    eup = "" if effective_unit_price_micro is None else money(effective_unit_price_micro)
    up = "" if unit_price_micro is None else money(unit_price_micro)
    sub = "" if subtotal_micro is None else money(subtotal_micro)

    return [
        PARTNER_ID,
        PARTNER_NAME,
        cust_id,
        cust_name,
        cust_domain,
        cust_country,
        invoice,
        "6001234",
        "0",
        order_id,
        charge_start,
        product_id,
        sku_id,
        availability_id,
        sku_name,
        sku_name,
        PUBLISHER,
        "",
        sku_name,
        subscription_id,
        charge_start,
        charge_end,
        term,
        eup,
        UNIT_TYPE,
        qty(quantity_units),
        sub,
        tax_total,
        sub,  # Total == Subtotal for tax-exempt partner billing
        CURRENCY,
        price_adjustment,
        ZERO_18,
        "",
        charge_type,
        up,
        qty(billable_units),
        CURRENCY,
        FX_RATE,
        FX_DATE,
        "",
        "",
        "",
        sub_start,
        sub_end,
        reference_id,
        "",
        promotion_id,
        PRODUCT_CATEGORY,
        drift,
    ]


def pick_class(rnd: random.Random) -> str:
    x = rnd.random()
    for name, ceiling in zip(CLASS_NAMES, CLASS_CUM):
        if x < ceiling:
            return name
    return NORMAL


def make_subscription(rnd: random.Random, sub_no: int) -> tuple[str, list[list[str]]]:
    """
    Emit every recon line belonging to one synthetic subscription.

    The line count is drawn *independently of the row class*, so the per-subscription
    class mix and the per-row class mix converge on the same percentages.
    """
    kind = pick_class(rnd)
    n_lines = rnd.choices([1, 2, 3, 4], weights=[45, 30, 17, 8])[0]

    stem = CUSTOMER_STEMS[rnd.randrange(len(CUSTOMER_STEMS))]
    cust_no = rnd.randrange(1, 141)  # ~140 tenants: a plausible mid-size direct CSP
    customer = (
        guid("c0000000", cust_no),
        f"{stem} {cust_no:03d}",
        f"{stem.split()[0].strip(',').lower()}{cust_no:03d}.onmicrosoft.com",
        COUNTRIES[cust_no % len(COUNTRIES)],
    )

    subscription_id = guid("50000000", sub_no)
    order_id = f"ORD-{sub_no:09d}"
    invoice = f"G{2026_0000 + rnd.randrange(1, 13):08d}"

    start_month = rnd.randrange(0, len(MONTHS) - n_lines) if n_lines < len(MONTHS) else 0
    sub_start_y, sub_start_m = MONTHS[start_month]
    sub_start = day_str(sub_start_y, sub_start_m, 1)

    if kind == EST_SUB:
        product = SUB_THRESHOLD_SKU
    elif kind == EST_3:
        product = CATALOG_MONTHLY[rnd.randrange(len(CATALOG_MONTHLY))]
    elif kind == EST_23:
        product = CATALOG_ANNUAL_ONLY[rnd.randrange(len(CATALOG_ANNUAL_ONLY))]
    else:
        product = CATALOG[rnd.randrange(len(CATALOG))]
    base = product[4]

    if kind == NORMAL:
        term = rnd.choices(
            [TERM_ANNUAL_MONTHLY, TERM_MONTHLY, TERM_ANNUAL_ANNUAL, TERM_TRIENNIAL],
            weights=[62, 25, 10, 3],
        )[0]
        eup = base
        seats = rnd.randrange(1, 260)
        sub_end = day_str(sub_start_y + 1, sub_start_m, 1)
        adjustment = ""
    else:
        # EST lands the subscription on a monthly term at an uplifted price.
        term = TERM_MONTHLY
        eup = base * EST_PCT[kind] // 100
        penalty_per_seat = eup - base
        if kind == EST_SUB:
            seats = 1  # $0.039/mo — deliberately under the $5/mo gate
        else:
            # Guarantee the monthly penalty clears the noise gate with room to spare.
            min_seats = NOISE_GATE_MICRO // penalty_per_seat + 2
            seats = rnd.randrange(min_seats, min_seats + 120)
        sub_end = day_str(sub_start_y, sub_start_m, calendar.monthrange(sub_start_y, sub_start_m)[1])
        # Microsoft does not always populate the description — 30 % of lines leave it
        # blank so the ratio-based fallback detection gets exercised.
        adjustment = "" if rnd.random() < 0.30 else (
            f"Extended Service Terms {EST_PCT[kind] - 100}% Fee Applied"
        )

    promotion_id = "PROMO-12M-2026" if kind == NORMAL and rnd.random() < 0.04 else ""

    rows: list[list[str]] = []
    for i in range(n_lines):
        year, month = MONTHS[(start_month + i) % len(MONTHS)]
        c_start, c_end, days = month_span(year, month)

        # ReferenceId: legacy scalar, post-June-2026 JSON object, or absent.
        r = rnd.random()
        if r < 0.45:
            reference_id = guid("8a7b6c5d", sub_no * 8 + i)
        elif r < 0.90:
            reference_id = (
                f'{{"osId":"OS-{sub_no % 100000:05d}",'
                f'"id":"{guid("8a7b6c5d", sub_no * 8 + i)}","v":2}}'
            )
        else:
            reference_id = ""

        charge_type = CHARGE_CYCLE_CASINGS[rnd.randrange(3)]
        quantity_units = seats
        billable_units = seats
        subtotal = eup * seats
        eup_cell: int | None = eup
        up_cell: int | None = base
        tax_total = SCI_ZERO if rnd.random() < 0.5 else ZERO_18
        drift = "future-value-we-do-not-model" if rnd.random() < 0.02 else ""

        if kind == NORMAL and i > 0:
            roll = rnd.random()
            if roll < 0.18:
                # Mid-cycle seat removal: negative billable quantity and credit.
                delta = rnd.randrange(1, max(2, seats // 4 + 2))
                start_day = rnd.randrange(2, days)
                c_start = day_str(year, month, start_day)
                remaining = days - start_day + 1
                charge_type = "removeQuantity"
                billable_units = -delta
                subtotal = -(eup * delta * remaining // days)
            elif roll < 0.34:
                delta = rnd.randrange(1, 12)
                start_day = rnd.randrange(2, days)
                c_start = day_str(year, month, start_day)
                remaining = days - start_day + 1
                charge_type = "addQuantity"
                quantity_units = seats + delta
                billable_units = delta
                subtotal = eup * delta * remaining // days
            elif roll < 0.38:
                charge_type = "cancelImmediate"
                billable_units = -seats
                subtotal = -eup * seats
            elif roll < 0.395:
                # Unknown charge type with blank prices: must be kept, not dropped.
                charge_type = UNKNOWN_CHARGE_TYPES[rnd.randrange(len(UNKNOWN_CHARGE_TYPES))]
                eup_cell = up_cell = subtotal = None
                quantity_units = billable_units = 0
                tax_total = ""

        rows.append(
            build_row(
                customer=customer,
                product=product,
                subscription_id=subscription_id,
                order_id=order_id,
                invoice=invoice,
                charge_start=c_start,
                charge_end=c_end,
                term=term,
                effective_unit_price_micro=eup_cell,
                unit_price_micro=up_cell,
                quantity_units=quantity_units,
                billable_units=billable_units,
                subtotal_micro=subtotal,
                charge_type=charge_type,
                tax_total=tax_total,
                price_adjustment=adjustment,
                reference_id=reference_id,
                promotion_id=promotion_id,
                sub_start=sub_start,
                sub_end=sub_end,
                drift=drift,
            )
        )

    return kind, rows


# --------------------------------------------------------------------------------------
# Streaming writer
# --------------------------------------------------------------------------------------


def render(rows: list[list[str]], header: list[str] | None) -> bytes:
    buf = io.StringIO(newline="")
    writer = csv.writer(buf, lineterminator="\r\n", quoting=csv.QUOTE_MINIMAL)
    if header is not None:
        writer.writerow(header)
    writer.writerows(rows)
    return buf.getvalue().encode("utf-8")


def human(n_bytes: int) -> str:
    return f"{n_bytes / MIB:,.0f} MB"


def generate(path: str, target_bytes: int, chunk_rows: int, seed: int, progress_mb: int) -> None:
    rnd = random.Random(seed)
    header = list(COLUMNS)
    header[0] = "\ufeff" + header[0]  # BOM welded onto the first cell, like the real export

    counts = {name: 0 for name in CLASS_NAMES}
    buffer: list[list[str]] = []
    written = 0
    rows_total = 0
    sub_no = 0
    next_progress = progress_mb * MIB
    target_mb = target_bytes // MIB
    avg_row = 800.0  # bootstrap guess, replaced by the real average after the first flush
    started = time.monotonic()

    with open(path, "wb", buffering=1 << 20) as fh:
        header_bytes = fh.write(render([], header))
        written += header_bytes

        while written < target_bytes:
            sub_no += 1
            kind, rows = make_subscription(rnd, sub_no)
            counts[kind] += len(rows)
            buffer.extend(rows)

            # Flush on a full chunk, or early once the buffered rows are estimated to
            # reach the target — otherwise a 50k-row chunk overshoots by ~37 MB.
            near_target = written + len(buffer) * avg_row >= target_bytes
            if len(buffer) >= chunk_rows or near_target:
                written += fh.write(render(buffer, None))
                rows_total += len(buffer)
                buffer.clear()
                avg_row = (written - header_bytes) / rows_total
                if written >= next_progress and not near_target:
                    pct = 100.0 * written / target_bytes
                    print(
                        f"Generated {written // MIB:>5,d} MB / {target_mb:,d} MB"
                        f"  ({pct:5.1f}%, {rows_total:,d} rows)",
                        flush=True,
                    )
                    next_progress = (written // (progress_mb * MIB) + 1) * progress_mb * MIB

        if buffer:
            written += fh.write(render(buffer, None))
            rows_total += len(buffer)
            buffer.clear()

    elapsed = time.monotonic() - started
    size = os.path.getsize(path)
    print()
    print(f"wrote {path}")
    print(f"  size      : {size:,d} bytes ({human(size)}, {size / 1e9:.2f} GB)")
    print(f"  rows      : {rows_total:,d} data rows + 1 header, {sub_no:,d} subscriptions")
    print(f"  columns   : {len(COLUMNS)} (48 canonical + {DRIFT_COLUMN})")
    print(f"  elapsed   : {elapsed:.1f}s ({rows_total / max(elapsed, 1e-9) / 1000:.0f}k rows/s)")
    print("  row mix   :")
    labels = {
        NORMAL: "normal traffic",
        EST_3: "EST +3%",
        EST_23: "EST +23%",
        EST_SUB: "EST below $5/mo gate",
    }
    for name in CLASS_NAMES:
        share = 100.0 * counts[name] / rows_total if rows_total else 0.0
        target = 100.0 * CLASS_WEIGHTS[name]
        print(f"    {labels[name]:<22} {counts[name]:>10,d}  {share:5.2f}%  (target {target:.1f}%)")


def main(argv: list[str]) -> int:
    repo_root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("-o", "--output", default=os.path.join(repo_root, "mock_recon_1gb.csv"),
                    help="output .csv path (default: <repo>/mock_recon_1gb.csv)")
    ap.add_argument("--size-mb", type=int, default=1000, help="target size in MiB (default: 1000)")
    ap.add_argument("--chunk-rows", type=int, default=50_000, help="rows buffered per disk write (default: 50000)")
    ap.add_argument("--seed", type=int, default=20260812, help="PRNG seed (default: 20260812)")
    ap.add_argument("--progress-mb", type=int, default=50, help="progress interval in MiB (default: 50)")
    args = ap.parse_args(argv)

    if args.size_mb <= 0 or args.chunk_rows <= 0 or args.progress_mb <= 0:
        ap.error("--size-mb, --chunk-rows and --progress-mb must be positive")

    out = os.path.abspath(args.output)
    os.makedirs(os.path.dirname(out), exist_ok=True)
    print(f"target {args.size_mb:,d} MB -> {out}  (seed {args.seed}, chunk {args.chunk_rows:,d} rows)")
    generate(out, args.size_mb * MIB, args.chunk_rows, args.seed, args.progress_mb)
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
