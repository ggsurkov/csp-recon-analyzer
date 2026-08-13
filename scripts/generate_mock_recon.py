#!/usr/bin/env python3
"""
Generate `fixtures/mock_recon_2026.csv.gz` — a synthetic Microsoft Partner Center
*license-based reconciliation* export shaped like the 2026 NCE schema.

This is the single source of truth for the parser fixture. It is deterministic:
running it twice produces byte-identical output (gzip mtime is pinned to 0).

Why a mock and not a real file: real recon exports contain partner + customer
tenant identifiers and purchase prices. Nothing here is real.

Deliberate edge cases baked into the output
-------------------------------------------
1.  UTF-8 BOM (\\ufeff) glued to the first header cell (`PartnerId`).
2.  Excel-guard leading apostrophe on every numeric cell: `'22.000000000000000000`.
3.  Scientific notation in `TaxTotal`: `'0E-20`.
4.  `ChargeType` in three different casings (`cycleCharge`, `cyclecharge`, `CycleCharge`)
    plus `removeQuantity`, `addQuantity`, `cancelImmediate` and an unknown
    `tierTransitionFee` the parser must not choke on.
5.  `ReferenceId` in both shapes: legacy scalar (pre-June-2026) and the
    JSON object `{"osId":"...","id":"...","v":2}` (post-June-2026). Plus empty.
6.  Negative `BillableQuantity` (`'-2.0000`) and negative `Subtotal` on mid-cycle
    `removeQuantity` prorations.
7.  A 49th column (`NewMicrosoftColumn2026`) that is NOT in our known schema —
    schema drift must be survivable, not fatal.
8.  A customer name containing a comma (`Fabrikam, Inc.`) → CSV quoting.
9.  CRLF line terminators, like the real export.
10. **Multi-member gzip**: the payload is written as two concatenated gzip members
    (rows 1-6, then rows 7-14) with the header only in the first member. Partner
    Center hands out multi-blob exports; naive `GzDecoder` silently reads only the
    first member and drops the rest. `MultiGzDecoder` is mandatory.

Findings the fixture is built to trigger
----------------------------------------
EST_UPLIFT   : S2 (+3% declared and corroborated, $6.00),
               S3 (+3% declared, UnitPrice is the pre-EST annual rate so the ratio does
                   not corroborate — $9.00, NOT the $59.00 a naive delta would claim),
               S10 (+3% inferred from the ratio alone, no prose, $18.00),
               S11 (+3%, $0.06 — below the $5/mo noise gate, must be suppressed)
DUPLICATE_SUB: Contoso holds M365 Business Premium on both S1 and S7
PROMO_EXPIRED: S8 Power BI Pro jumps 10.00 -> 14.00 between June and July

Usage:  python scripts/generate_mock_recon.py [-o fixtures/mock_recon_2026.csv.gz]
"""

from __future__ import annotations

import argparse
import csv
import gzip
import io
import os
import sys

# --------------------------------------------------------------------------------------
# Schema: the 48 canonical columns of the license-based recon export, plus one
# intentional unknown column to exercise schema-drift tolerance.
# --------------------------------------------------------------------------------------

KNOWN_COLUMNS = [
    "PartnerId",
    "PartnerName",
    "CustomerId",
    "CustomerName",
    "CustomerDomainName",
    "CustomerCountry",
    "InvoiceNumber",
    "MpnId",
    "Tier2MpnId",
    "OrderId",
    "OrderDate",
    "ProductId",
    "SkuId",
    "AvailabilityId",
    "SkuName",
    "ProductName",
    "PublisherName",
    "PublisherId",
    "SubscriptionDescription",
    "SubscriptionId",
    "ChargeStartDate",
    "ChargeEndDate",
    "TermAndBillingCycle",
    "EffectiveUnitPrice",
    "UnitType",
    "Quantity",
    "Subtotal",
    "TaxTotal",
    "Total",
    "Currency",
    "PriceAdjustmentDescription",
    "PublisherDiscount",
    "DiscountDetails",
    "ChargeType",
    "UnitPrice",
    "BillableQuantity",
    "PricingCurrency",
    "PCToBCExchangeRate",
    "PCToBCExchangeRateDate",
    "MeterDescription",
    "ReservationOrderId",
    "CreditReasonCode",
    "SubscriptionStartDate",
    "SubscriptionEndDate",
    "ReferenceId",
    "ProductQualifiers",
    "PromotionId",
    "ProductCategory",
]
assert len(KNOWN_COLUMNS) == 48, f"expected 48 canonical columns, got {len(KNOWN_COLUMNS)}"

DRIFT_COLUMN = "NewMicrosoftColumn2026"
COLUMNS = KNOWN_COLUMNS + [DRIFT_COLUMN]

# --------------------------------------------------------------------------------------
# Fixed values shared by every row
# --------------------------------------------------------------------------------------

PARTNER_ID = "a1b2c3d4-0000-4000-8000-000000000001"
PARTNER_NAME = "Northstar Managed Services"
INVOICE = "G000123456"
MPN_ID = "6001234"
TIER2_MPN_ID = "0"
CURRENCY = "USD"
FX_RATE = "'1.000000000000000000"
FX_DATE = "2026-07-01T00:00:00Z"
ZERO_18 = "'0.000000000000000000"
SCI_ZERO = "'0E-20"  # Microsoft emits this in TaxTotal for tax-exempt lines

CUSTOMERS = {
    "contoso": (
        "11111111-1111-4111-8111-111111111111",
        "Contoso Ltd",
        "contoso.onmicrosoft.com",
        "US",
    ),
    # Comma in the name on purpose: forces CSV quoting.
    "fabrikam": (
        "22222222-2222-4222-8222-222222222222",
        "Fabrikam, Inc.",
        "fabrikam.onmicrosoft.com",
        "US",
    ),
    "northwind": (
        "33333333-3333-4333-8333-333333333333",
        "Northwind Traders",
        "northwind.onmicrosoft.com",
        "CA",
    ),
}

# ProductId / SkuId / AvailabilityId / SkuName / ProductName
CATALOG = {
    "m365bp": ("CFQ7TTC0LCHC", "0002", "DZH318Z0BQ4B", "Microsoft 365 Business Premium", "Microsoft 365 Business Premium"),
    "exo_p1": ("CFQ7TTC0LH16", "0001", "DZH318Z0BQ3Q", "Exchange Online (Plan 1)", "Exchange Online"),
    "proj_p3": ("CFQ7TTC0HDB1", "0002", "DZH318Z0BQ4P", "Project Plan 3", "Project Plan 3"),
    "teams_phone": ("CFQ7TTC0MSSN", "0001", "DZH318Z0BQ5C", "Microsoft Teams Phone Standard", "Microsoft Teams Phone Standard"),
    "pbi_pro": ("CFQ7TTC0L3PB", "0002", "DZH318Z0BQ6F", "Power BI Pro", "Power BI Pro"),
    "visio_p2": ("CFQ7TTC0HD33", "0002", "DZH318Z0BQ7H", "Visio Plan 2", "Visio Plan 2"),
    "defender": ("CFQ7TTC0LH0X", "0001", "DZH318Z0BQ8J", "Microsoft Defender for Office 365 (Plan 1)", "Microsoft Defender for Office 365"),
}


def money(value: str) -> str:
    """Excel-guarded, 18-decimal money cell exactly as Partner Center writes it."""
    return "'" + value


def qty(value: str) -> str:
    """Excel-guarded, 4-decimal quantity cell."""
    return "'" + value


def row(
    *,
    customer: str,
    product: str,
    subscription_id: str,
    order_id: str,
    charge_start: str,
    charge_end: str,
    term: str,
    effective_unit_price: str,
    unit_price: str,
    quantity: str,
    billable_quantity: str,
    subtotal: str,
    total: str,
    charge_type: str,
    tax_total: str = ZERO_18,
    price_adjustment: str = "",
    reference_id: str = "",
    promotion_id: str = "",
    sub_start: str = "2025-08-01T00:00:00Z",
    sub_end: str = "2026-07-31T00:00:00Z",
    drift: str = "",
) -> list[str]:
    cust_id, cust_name, cust_domain, cust_country = CUSTOMERS[customer]
    product_id, sku_id, availability_id, sku_name, product_name = CATALOG[product]
    return [
        PARTNER_ID,
        PARTNER_NAME,
        cust_id,
        cust_name,
        cust_domain,
        cust_country,
        INVOICE,
        MPN_ID,
        TIER2_MPN_ID,
        order_id,
        "2026-06-28T00:00:00Z",
        product_id,
        sku_id,
        availability_id,
        sku_name,
        product_name,
        "Microsoft",
        "",
        sku_name,
        subscription_id,
        charge_start,
        charge_end,
        term,
        money(effective_unit_price),
        "Licenses",
        qty(quantity),
        money(subtotal),
        tax_total,
        money(total),
        CURRENCY,
        price_adjustment,
        ZERO_18,
        "",
        charge_type,
        money(unit_price),
        qty(billable_quantity),
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
        "license-based",
        drift,
    ]


# Subscription identifiers, kept readable so assertions in tests can name them.
S1 = "11110000-0000-4000-8000-000000000001"   # Contoso  M365 BP        (baseline)
S2 = "11110000-0000-4000-8000-000000000002"   # Contoso  EXO P1         (EST +3%)
S3 = "22220000-0000-4000-8000-000000000003"   # Fabrikam Project P3     (EST +23%)
S4 = "22220000-0000-4000-8000-000000000004"   # Fabrikam M365 BP        (prorations)
S5 = "33330000-0000-4000-8000-000000000005"   # Northwind Teams Phone   (ReferenceId v2)
S6 = "33330000-0000-4000-8000-000000000006"   # Northwind Defender      (cancelImmediate)
S7 = "11110000-0000-4000-8000-000000000007"   # Contoso  M365 BP        (duplicate of S1)
S8 = "11110000-0000-4000-8000-000000000008"   # Contoso  Power BI Pro   (promo expiry)
S9 = "33330000-0000-4000-8000-000000000009"   # Northwind Visio P2      (annual term)
S10 = "33330000-0000-4000-8000-000000000010"  # Northwind Teams Phone   (EST +23%, implicit)
S11 = "11110000-0000-4000-8000-000000000011"  # Contoso  EXO P1         (EST +3%, sub-$5)
S12 = "33330000-0000-4000-8000-000000000012"  # Northwind Visio P2      (unknown charge type)

FULL_MONTH = ("2026-07-01T00:00:00Z", "2026-07-31T00:00:00Z")
MID_CYCLE = ("2026-07-12T00:00:00Z", "2026-07-31T00:00:00Z")

ROWS: list[list[str]] = [
    # 1. Plain NCE cycle charge. Annual term billed monthly. TaxTotal in scientific zero.
    row(
        customer="contoso", product="m365bp", subscription_id=S1, order_id="ORD-0001",
        charge_start=FULL_MONTH[0], charge_end=FULL_MONTH[1],
        term="Annual term, Monthly billing",
        effective_unit_price="22.000000000000000000", unit_price="22.000000000000000000",
        quantity="25.0000", billable_quantity="25.0000",
        subtotal="550.000000000000000000", total="550.000000000000000000",
        charge_type="cycleCharge", tax_total=SCI_ZERO,
        reference_id="8a7b6c5d-0000-4000-8000-0000000000a1",
    ),
    # 2. EST +3%: SKU has a monthly plan, subscription rolled onto a monthly term.
    #    4.00 -> 4.12 across 50 seats = $6.00/mo of pure penalty.
    row(
        customer="contoso", product="exo_p1", subscription_id=S2, order_id="ORD-0002",
        charge_start=FULL_MONTH[0], charge_end=FULL_MONTH[1],
        term="Monthly term, Monthly billing",
        effective_unit_price="4.120000000000000000", unit_price="4.000000000000000000",
        quantity="50.0000", billable_quantity="50.0000",
        subtotal="206.000000000000000000", total="206.000000000000000000",
        charge_type="cycleCharge",
        price_adjustment="Extended Service Terms 3% Fee Applied",
        reference_id="8a7b6c5d-0000-4000-8000-0000000000a2",
    ),
    # 3. EST declared, but UnitPrice still carries the pre-EST *annual* rate (25.00) rather
    #    than monthly list. The line therefore sits 23.6% above its own UnitPrice — which is
    #    the lost annual discount, not a Microsoft fee. Monthly list is 30.90 / 1.03 = 30.00,
    #    so the surcharge is 0.90/seat = $9.00/mo. A detector reading effective - UnitPrice
    #    as the penalty would claim $59.00. Lowercase charge type on purpose.
    row(
        customer="fabrikam", product="proj_p3", subscription_id=S3, order_id="ORD-0003",
        charge_start=FULL_MONTH[0], charge_end=FULL_MONTH[1],
        term="Monthly term, Monthly billing",
        effective_unit_price="30.900000000000000000", unit_price="25.000000000000000000",
        quantity="10.0000", billable_quantity="10.0000",
        subtotal="309.000000000000000000", total="309.000000000000000000",
        charge_type="cyclecharge",
        price_adjustment="Extended Service Terms 3% Fee Applied",
        reference_id="8a7b6c5d-0000-4000-8000-0000000000a3",
    ),
    # 4. Mid-cycle removeQuantity: 2 seats dropped on day 12 of a 31-day cycle.
    #    22.00 * 20/31 * 2 = 28.387096774193548387, credited back.
    row(
        customer="fabrikam", product="m365bp", subscription_id=S4, order_id="ORD-0004",
        charge_start=MID_CYCLE[0], charge_end=MID_CYCLE[1],
        term="Annual term, Monthly billing",
        effective_unit_price="22.000000000000000000", unit_price="22.000000000000000000",
        quantity="25.0000", billable_quantity="-2.0000",
        subtotal="-28.387096774193548387", total="-28.387096774193548387",
        charge_type="removeQuantity", tax_total=SCI_ZERO,
        reference_id="8a7b6c5d-0000-4000-8000-0000000000a4",
    ),
    # 5. Mid-cycle addQuantity on the same subscription: 3 seats added on day 12.
    row(
        customer="fabrikam", product="m365bp", subscription_id=S4, order_id="ORD-0005",
        charge_start=MID_CYCLE[0], charge_end=MID_CYCLE[1],
        term="Annual term, Monthly billing",
        effective_unit_price="22.000000000000000000", unit_price="22.000000000000000000",
        quantity="28.0000", billable_quantity="3.0000",
        subtotal="42.580645161290322580", total="42.580645161290322580",
        charge_type="addQuantity",
        reference_id="8a7b6c5d-0000-4000-8000-0000000000a5",
    ),
    # 6. Post-June-2026 ReferenceId shape + mixed-case charge type.
    row(
        customer="northwind", product="teams_phone", subscription_id=S5, order_id="ORD-0006",
        charge_start=FULL_MONTH[0], charge_end=FULL_MONTH[1],
        term="Annual term, Monthly billing",
        effective_unit_price="8.000000000000000000", unit_price="8.000000000000000000",
        quantity="12.0000", billable_quantity="12.0000",
        subtotal="96.000000000000000000", total="96.000000000000000000",
        charge_type="CycleCharge", tax_total=SCI_ZERO,
        reference_id='{"osId":"OS-77341","id":"8a7b6c5d-0000-4000-8000-0000000000a6","v":2}',
        drift="future-value-we-do-not-model",
    ),
    # ---- gzip member boundary falls here ----
    # 7. Instant cancellation inside the cancellation window: full credit back.
    row(
        customer="northwind", product="defender", subscription_id=S6, order_id="ORD-0007",
        charge_start=FULL_MONTH[0], charge_end=FULL_MONTH[1],
        term="Annual term, Monthly billing",
        effective_unit_price="1.500000000000000000", unit_price="1.500000000000000000",
        quantity="40.0000", billable_quantity="-40.0000",
        subtotal="-60.000000000000000000", total="-60.000000000000000000",
        charge_type="cancelImmediate",
        reference_id='{"osId":"OS-77342","id":"8a7b6c5d-0000-4000-8000-0000000000a7","v":2}',
    ),
    # 8. DUPLICATE_SUBSCRIPTION: Contoso pays for M365 BP twice (S1 above and S7 here),
    #    the classic leftover after an NCE migration.
    row(
        customer="contoso", product="m365bp", subscription_id=S7, order_id="ORD-0008",
        charge_start=FULL_MONTH[0], charge_end=FULL_MONTH[1],
        term="Annual term, Monthly billing",
        effective_unit_price="22.000000000000000000", unit_price="22.000000000000000000",
        quantity="25.0000", billable_quantity="25.0000",
        subtotal="550.000000000000000000", total="550.000000000000000000",
        charge_type="cycleCharge",
        reference_id="8a7b6c5d-0000-4000-8000-0000000000a8",
    ),
    # 9. Power BI Pro under a 12-month promo — June period, discounted price.
    row(
        customer="contoso", product="pbi_pro", subscription_id=S8, order_id="ORD-0009",
        charge_start="2026-06-01T00:00:00Z", charge_end="2026-06-30T00:00:00Z",
        term="Annual term, Monthly billing",
        effective_unit_price="10.000000000000000000", unit_price="14.000000000000000000",
        quantity="30.0000", billable_quantity="30.0000",
        subtotal="300.000000000000000000", total="300.000000000000000000",
        charge_type="cycleCharge",
        promotion_id="PROMO-PBIPRO-12M",
        reference_id="8a7b6c5d-0000-4000-8000-0000000000a9",
    ),
    # 10. PROMO_EXPIRED: same subscription, same seat count, July price back to list.
    row(
        customer="contoso", product="pbi_pro", subscription_id=S8, order_id="ORD-0009",
        charge_start=FULL_MONTH[0], charge_end=FULL_MONTH[1],
        term="Annual term, Monthly billing",
        effective_unit_price="14.000000000000000000", unit_price="14.000000000000000000",
        quantity="30.0000", billable_quantity="30.0000",
        subtotal="420.000000000000000000", total="420.000000000000000000",
        charge_type="cycleCharge",
        reference_id="8a7b6c5d-0000-4000-8000-0000000000aa",
    ),
    # 11. Annual term billed annually — seat reduction here is AT_RENEWAL only.
    row(
        customer="northwind", product="visio_p2", subscription_id=S9, order_id="ORD-0011",
        charge_start="2026-07-01T00:00:00Z", charge_end="2027-06-30T00:00:00Z",
        term="Annual term, Annual billing",
        effective_unit_price="180.000000000000000000", unit_price="180.000000000000000000",
        quantity="6.0000", billable_quantity="6.0000",
        subtotal="1080.000000000000000000", total="1080.000000000000000000",
        charge_type="cycleCharge",
        sub_start="2026-07-01T00:00:00Z", sub_end="2027-06-30T00:00:00Z",
        reference_id="8a7b6c5d-0000-4000-8000-0000000000ab",
    ),
    # 12. EST +3% with NO PriceAdjustmentDescription — Microsoft does not always populate
    #     it. Detection must fall back to the 15.00 -> 15.45 price ratio, at the reduced
    #     confidence that inference earns. 0.45 * 40 = $18.00/mo.
    row(
        customer="northwind", product="teams_phone", subscription_id=S10, order_id="ORD-0012",
        charge_start=FULL_MONTH[0], charge_end=FULL_MONTH[1],
        term="Monthly term, Monthly billing",
        effective_unit_price="15.450000000000000000", unit_price="15.000000000000000000",
        quantity="40.0000", billable_quantity="40.0000",
        subtotal="618.000000000000000000", total="618.000000000000000000",
        charge_type="cycleCharge",
        reference_id="8a7b6c5d-0000-4000-8000-0000000000ac",
    ),
    # 13. EST +3% worth $0.06/mo — real, but below the $5/mo noise gate. Must be
    #     found by the detector and suppressed from the reported total.
    row(
        customer="contoso", product="exo_p1", subscription_id=S11, order_id="ORD-0013",
        charge_start=FULL_MONTH[0], charge_end=FULL_MONTH[1],
        term="Monthly term, Monthly billing",
        effective_unit_price="2.060000000000000000", unit_price="2.000000000000000000",
        quantity="1.0000", billable_quantity="1.0000",
        subtotal="2.060000000000000000", total="2.060000000000000000",
        charge_type="cycleCharge",
        price_adjustment="Extended Service Terms 3% Fee Applied",
        reference_id="8a7b6c5d-0000-4000-8000-0000000000ad",
    ),
    # 14. Unknown charge type, blank ReferenceId, blank prices. The parser must keep
    #     the row rather than drop it, and surface the charge type verbatim.
    row(
        customer="northwind", product="visio_p2", subscription_id=S12, order_id="ORD-0014",
        charge_start=FULL_MONTH[0], charge_end=FULL_MONTH[1],
        term="Annual term, Monthly billing",
        effective_unit_price="", unit_price="",
        quantity="0.0000", billable_quantity="0.0000",
        subtotal="", total="",
        charge_type="tierTransitionFee", tax_total="",
        reference_id="",
    ),
]

MEMBER_SPLIT = 6  # rows 1..6 in gzip member #1, the rest in member #2


def render_csv(rows: list[list[str]], header: list[str] | None) -> bytes:
    """Serialise to CRLF-terminated, minimally-quoted CSV bytes."""
    buf = io.StringIO(newline="")
    writer = csv.writer(buf, lineterminator="\r\n", quoting=csv.QUOTE_MINIMAL)
    if header is not None:
        writer.writerow(header)
    writer.writerows(rows)
    return buf.getvalue().encode("utf-8")


def build() -> bytes:
    """Two concatenated gzip members, header only in the first."""
    header = list(COLUMNS)
    header[0] = "﻿" + header[0]  # BOM welded onto the first cell

    part1 = render_csv(ROWS[:MEMBER_SPLIT], header)
    part2 = render_csv(ROWS[MEMBER_SPLIT:], None)

    out = io.BytesIO()
    for payload in (part1, part2):
        # mtime=0 keeps the output byte-reproducible across runs.
        with gzip.GzipFile(fileobj=out, mode="wb", compresslevel=9, mtime=0) as gz:
            gz.write(payload)
    return out.getvalue()


def main(argv: list[str]) -> int:
    default_out = os.path.join(
        os.path.dirname(os.path.dirname(os.path.abspath(__file__))),
        "fixtures",
        "mock_recon_2026.csv.gz",
    )
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("-o", "--output", default=default_out, help="output .csv.gz path")
    ap.add_argument("--plain", action="store_true", help="also write the uncompressed .csv next to it")
    args = ap.parse_args(argv)

    os.makedirs(os.path.dirname(os.path.abspath(args.output)), exist_ok=True)
    blob = build()
    with open(args.output, "wb") as fh:
        fh.write(blob)

    if args.plain:
        plain_path = args.output[:-3] if args.output.endswith(".gz") else args.output + ".csv"
        header = list(COLUMNS)
        header[0] = "﻿" + header[0]
        with open(plain_path, "wb") as fh:
            fh.write(render_csv(ROWS, header))
        print(f"wrote {plain_path}")

    print(f"wrote {args.output} ({len(blob)} bytes, {len(ROWS)} rows, 2 gzip members)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
