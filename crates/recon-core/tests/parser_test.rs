//! End-to-end tests against `fixtures/mock_recon_2026.csv.gz`.
//!
//! The fixture is committed. If it is missing, these tests try to regenerate it with
//! `scripts/generate_mock_recon.py` and skip with a clear message if Python is unavailable
//! — a missing interpreter should not read as a code failure.
//!
//! Every expected number here is arithmetic that can be checked by hand against the row
//! table in the generator script. That is deliberate: a test whose expectations were copied
//! out of the implementation's own output proves nothing.

use std::fs::File;
use std::path::{Path, PathBuf};
use std::process::Command;

use recon_core::detectors::est::{EstDetector, EstEvidenceKind, EstPolicy};
use recon_core::types::{BillingCycle, TermDuration};
use recon_core::{
    stream_recon_auto, stream_recon_gz, ChargeType, Decimal, ParseOptions, ReconRow, ReferenceId,
    RemediationWindow, StreamStats,
};
use rust_decimal_macros::dec;

const EXPECTED_ROWS: u64 = 14;

fn repo_root() -> PathBuf {
    // crates/recon-core -> crates -> repo root
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crate lives two levels below the repo root")
        .to_path_buf()
}

/// Path to the fixture, regenerating it from the Python script if it is absent.
fn fixture_path() -> Option<PathBuf> {
    let root = repo_root();
    let fixture = root.join("fixtures").join("mock_recon_2026.csv.gz");
    if fixture.is_file() {
        return Some(fixture);
    }

    let script = root.join("scripts").join("generate_mock_recon.py");
    for python in ["python3", "python", "py"] {
        match Command::new(python).arg(&script).arg("-o").arg(&fixture).status() {
            Ok(s) if s.success() && fixture.is_file() => return Some(fixture),
            _ => continue,
        }
    }

    eprintln!(
        "SKIP: {} is missing and no Python interpreter could regenerate it.\n      \
         Run: python scripts/generate_mock_recon.py",
        fixture.display()
    );
    None
}

/// Read the whole fixture into memory. Returns `None` when the fixture is unavailable.
fn load() -> Option<(Vec<ReconRow>, StreamStats)> {
    let path = fixture_path()?;
    let file = File::open(&path).expect("fixture is readable");

    let mut rows = Vec::new();
    let opts = ParseOptions { capture_unknown_columns: true, ..Default::default() };
    let stats = stream_recon_gz(file, &opts, |row| rows.push(row.clone())).expect("fixture parses");
    Some((rows, stats))
}

/// Skips the test body when the fixture cannot be produced.
macro_rules! fixture {
    () => {
        match load() {
            Some(v) => v,
            None => return,
        }
    };
}

fn find(rows: &[ReconRow], subscription_suffix: &str) -> Vec<ReconRow> {
    rows.iter().filter(|r| r.subscription_id.ends_with(subscription_suffix)).cloned().collect()
}

// -------------------------------------------------------------------------------------
// Parsing
// -------------------------------------------------------------------------------------

#[test]
fn every_row_parses_across_both_gzip_members() {
    let (rows, stats) = fixture!();

    // The decisive assertion: rows 7..14 live in the *second* gzip member. A plain
    // GzDecoder returns 6 rows here and no error at all.
    assert_eq!(stats.rows_parsed, EXPECTED_ROWS, "multi-member gzip was not fully read");
    assert_eq!(rows.len() as u64, EXPECTED_ROWS);
    assert_eq!(stats.rows_failed, 0);
    assert_eq!(stats.row_error_count, 0, "unexpected cell errors: {:?}", stats.row_errors);
    assert!(stats.is_clean());

    // Record indices are contiguous and usable as evidence anchors.
    for (i, row) in rows.iter().enumerate() {
        assert_eq!(row.record_index, i as u64);
    }
}

#[test]
fn the_bom_does_not_swallow_the_first_column() {
    let (rows, _) = fixture!();
    // If the BOM were left on the header, PartnerId would bind to nothing and every row
    // would carry an empty partner id.
    assert!(rows.iter().all(|r| r.partner_id == "a1b2c3d4-0000-4000-8000-000000000001"));
}

#[test]
fn schema_drift_is_reported_and_survivable() {
    let (rows, stats) = fixture!();

    assert_eq!(stats.unknown_columns, vec!["NewMicrosoftColumn2026"]);
    assert!(stats.missing_columns.is_empty(), "missing: {:?}", stats.missing_columns);

    let with_value = rows.iter().find(|r| r.subscription_id.ends_with("0005")).unwrap();
    assert_eq!(
        with_value.unknown,
        vec![("NewMicrosoftColumn2026".to_string(), "future-value-we-do-not-model".to_string())]
    );
}

#[test]
fn quoted_fields_and_embedded_commas_survive() {
    let (rows, _) = fixture!();
    assert!(rows.iter().any(|r| r.customer_name == "Fabrikam, Inc."));
    assert!(rows.iter().any(|r| r.term_and_billing_cycle == "Annual term, Monthly billing"));
}

#[test]
fn excel_guarded_and_scientific_numbers_parse_exactly() {
    let (rows, _) = fixture!();

    let baseline = &find(&rows, "0001")[0];
    assert_eq!(baseline.effective_unit_price, Some(dec!(22)));
    assert_eq!(baseline.billable_quantity, Some(dec!(25)));
    assert_eq!(baseline.subtotal, Some(dec!(550)));
    // `'0E-20` — scientific notation that `Decimal::from_str` alone rejects.
    assert_eq!(baseline.tax_total, Some(Decimal::ZERO));

    // 18 decimals held exactly, no float rounding anywhere on the path.
    let removal = find(&rows, "0004")
        .into_iter()
        .find(|r| r.charge_type == ChargeType::RemoveQuantity)
        .unwrap();
    assert_eq!(removal.subtotal.unwrap().to_string(), "-28.387096774193548387");
}

#[test]
fn charge_types_parse_regardless_of_casing() {
    let (rows, _) = fixture!();

    // cycleCharge / cyclecharge / CycleCharge all appear in the fixture.
    let cycles = rows.iter().filter(|r| r.charge_type == ChargeType::CycleCharge).count();
    assert_eq!(cycles, 10);

    // An unmodelled charge type is preserved verbatim rather than guessed at.
    let unknown = &find(&rows, "0012")[0];
    assert_eq!(unknown.charge_type, ChargeType::Other("tierTransitionFee".into()));
    assert_eq!(unknown.charge_type.sign(), 1, "unknown types must not be assumed to be credits");
    // Blank price cells are absent, not zero.
    assert_eq!(unknown.effective_unit_price, None);
    assert_eq!(unknown.reference_id, ReferenceId::Absent);
}

#[test]
fn negative_quantities_on_credits_are_not_double_negated() {
    let (rows, _) = fixture!();

    let removal = find(&rows, "0004")
        .into_iter()
        .find(|r| r.charge_type == ChargeType::RemoveQuantity)
        .unwrap();
    assert_eq!(removal.billable_quantity, Some(dec!(-2)));
    // Naively multiplying by ChargeType::sign() would yield +2 and flip a credit into a charge.
    assert_eq!(removal.signed_billable_quantity(), Some(dec!(-2)));
    assert_eq!(removal.extended_cost(), Some(dec!(-44))); // 22.00 * -2, full-period rate
    assert_eq!(removal.charge_days(), Some(20)); // 12th..31st inclusive

    // The addQuantity sibling on the same subscription stays positive.
    let addition =
        find(&rows, "0004").into_iter().find(|r| r.charge_type == ChargeType::AddQuantity).unwrap();
    assert_eq!(addition.signed_billable_quantity(), Some(dec!(3)));

    // A whole-subscription cancellation, already negative in the file.
    let cancel = &find(&rows, "0006")[0];
    assert_eq!(cancel.charge_type, ChargeType::CancelImmediate);
    assert_eq!(cancel.signed_billable_quantity(), Some(dec!(-40)));
    assert_eq!(cancel.subtotal, Some(dec!(-60)));

    // Net seat movement on the prorated subscription: -2 + 3 = +1.
    let net: Decimal =
        find(&rows, "0004").iter().filter_map(|r| r.signed_billable_quantity()).sum();
    assert_eq!(net, dec!(1));
}

#[test]
fn reference_id_reads_both_the_legacy_and_the_v2_shape() {
    let (rows, _) = fixture!();

    let legacy = &find(&rows, "0001")[0];
    assert_eq!(
        legacy.reference_id,
        ReferenceId::Legacy("8a7b6c5d-0000-4000-8000-0000000000a1".into())
    );

    let v2 = &find(&rows, "0005")[0];
    assert_eq!(
        v2.reference_id,
        ReferenceId::V2 {
            os_id: Some("OS-77341".into()),
            id: Some("8a7b6c5d-0000-4000-8000-0000000000a6".into()),
            v: 2,
        }
    );
    // Both shapes answer the same question.
    assert_eq!(v2.reference_id.id(), Some("8a7b6c5d-0000-4000-8000-0000000000a6"));
    assert!(rows.iter().any(|r| r.reference_id.is_absent()));
}

#[test]
fn terms_are_split_into_commitment_and_billing_cycle() {
    let (rows, _) = fixture!();

    let monthly_billed_annual = &find(&rows, "0001")[0];
    assert_eq!(monthly_billed_annual.term.duration, TermDuration::Annual);
    assert_eq!(monthly_billed_annual.term.cycle, BillingCycle::Monthly);

    let annual = &find(&rows, "0009")[0];
    assert_eq!(annual.term.duration, TermDuration::Annual);
    assert_eq!(annual.term.cycle, BillingCycle::Annual);
    assert_eq!(annual.charge_days(), Some(365));

    let est_monthly = &find(&rows, "0002")[0];
    assert_eq!(est_monthly.term.duration, TermDuration::Monthly);
}

#[test]
fn the_gzip_sniffer_reaches_the_same_answer() {
    let path = match fixture_path() {
        Some(p) => p,
        None => return,
    };
    let file = File::open(path).unwrap();
    let mut n = 0u64;
    let stats = stream_recon_auto(file, &ParseOptions::default(), |_| n += 1).unwrap();
    assert_eq!((n, stats.rows_parsed), (EXPECTED_ROWS, EXPECTED_ROWS));
}

// -------------------------------------------------------------------------------------
// EST detector
// -------------------------------------------------------------------------------------

/// Run the detector over the fixture the way production does: one pass, streaming.
fn est_report() -> Option<recon_core::detectors::est::EstReport> {
    let path = fixture_path()?;
    let file = File::open(path).expect("fixture is readable");
    let mut est = EstDetector::new();
    stream_recon_gz(file, &ParseOptions::default(), |row| est.observe(row)).expect("parses");
    Some(est.finish())
}

/// S3 is the line where `UnitPrice` is not monthly list, and it is the whole reason the
/// `1.23` band was removed.
///
/// Project Plan 3 charged at 30.90 against a `UnitPrice` of 25.00 — the pre-EST annual rate.
/// The naive reading is a 23.6% penalty worth 59.00/month. The truth is that monthly list is
/// 30.90 / 1.03 = 30.00 and Microsoft surcharged 0.90/seat, so 9.00/month. The rest of that
/// gap is the annual discount the partner lost, which is real money and is not a fee.
#[test]
fn est_claims_only_the_surcharge_when_unit_price_is_not_monthly_list() {
    let report = match est_report() {
        Some(r) => r,
        None => return,
    };

    let f = report
        .findings
        .iter()
        .find(|f| f.subscription_id.ends_with("0003"))
        .expect("the declared EST line must be found");

    assert_eq!(f.rate, dec!(0.03));
    assert_eq!(f.reason, "Extended Service Term surcharge over monthly list price");
    assert_eq!(f.effective_unit_price, dec!(30.90));
    // Derived from the surcharge, not read from `UnitPrice`.
    assert_eq!(f.base_unit_price, dec!(30.00));
    assert_eq!(f.uplift_per_seat, dec!(0.90));
    assert_eq!(f.billable_quantity, dec!(10));
    assert_eq!(f.uplift_amount, dec!(9.00));
    assert_eq!(f.monthly_run_rate, dec!(9.00));
    // Declared by Microsoft, but the file's own list price does not corroborate it.
    assert_eq!(f.evidence_kind, EstEvidenceKind::DeclaredNotCorroborated);
    assert_eq!(f.confidence, dec!(0.90));
    assert_eq!(f.customer_name, "Fabrikam, Inc.");
    assert_eq!(f.sku_name, "Project Plan 3");
    // EST rolls the subscription onto a monthly term, so this is fixable today.
    assert_eq!(f.remediation_window, RemediationWindow::Now);
    // The finding points back at a real row.
    assert_eq!(f.record_index, 2);
}

#[test]
fn est_finds_the_three_percent_penalty() {
    let report = match est_report() {
        Some(r) => r,
        None => return,
    };

    // S2: Exchange Online P1, 4.00 -> 4.12, 50 seats = 6.00/month.
    let f = report.findings.iter().find(|f| f.subscription_id.ends_with("0002")).expect("3% line");
    assert_eq!(f.rate, dec!(0.03));
    assert_eq!(f.reason, "Extended Service Term surcharge over monthly list price");
    // Here `UnitPrice` really is monthly list, so it is used rather than derived.
    assert_eq!(f.base_unit_price, dec!(4.00));
    assert_eq!(f.uplift_per_seat, dec!(0.12));
    assert_eq!(f.uplift_amount, dec!(6.00));
    assert_eq!(f.evidence_kind, EstEvidenceKind::DeclaredAndPriced);
    assert_eq!(f.confidence, dec!(1.00));
}

#[test]
fn est_detects_an_undeclared_uplift_from_the_price_ratio_alone() {
    let report = match est_report() {
        Some(r) => r,
        None => return,
    };

    // S10 carries no PriceAdjustmentDescription: 15.00 -> 15.45 across 40 seats = 18.00.
    let f =
        report.findings.iter().find(|f| f.subscription_id.ends_with("0010")).expect("ratio line");
    assert_eq!(f.rate, dec!(0.03));
    assert_eq!(f.uplift_amount, dec!(18.00));
    assert_eq!(f.evidence_kind, EstEvidenceKind::PriceRatio);
    assert!(f.price_adjustment_description.is_empty());
    // Ratio-only is suggestive, not proof, and must say so rather than claim certainty.
    assert_eq!(f.confidence, dec!(0.75));
}

#[test]
fn est_totals_are_exact_and_the_noise_gate_holds() {
    let report = match est_report() {
        Some(r) => r,
        None => return,
    };

    // Four EST lines in the file: 6.00 + 9.00 + 18.00 + 0.06.
    assert_eq!(report.findings.len(), 4);
    assert_eq!(report.subscriptions.len(), 4);

    // 6.00 + 9.00 + 18.00 — S11's 0.06/month is below the $5 gate.
    assert_eq!(report.total_monthly_run_rate, dec!(33.00));
    assert_eq!(report.total_uplift_amount, dec!(33.00));
    assert_eq!(report.noise_threshold_monthly, dec!(5));

    assert_eq!(report.suppressed_subscription_count, 1);
    assert_eq!(report.suppressed_monthly_run_rate, dec!(0.06));
    // Suppressed, not deleted: the row is still in the report for anyone who looks.
    assert!(report.findings.iter().any(|f| f.subscription_id.ends_with("0011")));
    assert_eq!(report.reportable().count(), 3);

    // Ranked by what it costs, largest first.
    let ranked: Vec<Decimal> = report.reportable().map(|s| s.monthly_run_rate).collect();
    assert_eq!(ranked, vec![dec!(18.00), dec!(9.00), dec!(6.00)]);
}

#[test]
fn est_leaves_ordinary_lines_alone() {
    let report = match est_report() {
        Some(r) => r,
        None => return,
    };
    let flagged: Vec<&str> = report.findings.iter().map(|f| f.subscription_id.as_str()).collect();

    // Baseline cycle charge at list price.
    assert!(!flagged.iter().any(|s| s.ends_with("0001")));
    // Prorations at list price.
    assert!(!flagged.iter().any(|s| s.ends_with("0004")));
    // An expired promo is a 40% jump — a real finding, but not this detector's.
    assert!(!flagged.iter().any(|s| s.ends_with("0008")));
    // Annual commitment at list price.
    assert!(!flagged.iter().any(|s| s.ends_with("0009")));
}

#[test]
fn est_respects_the_policy_effective_date() {
    let path = match fixture_path() {
        Some(p) => p,
        None => return,
    };

    // Pin the policy to a date after every charge in the file: nothing may be flagged.
    let mut future = EstPolicy::current();
    future.effective_from = chrono::NaiveDate::from_ymd_opt(2027, 1, 1).unwrap();

    let file = File::open(path).unwrap();
    let mut est = EstDetector::with_policy(future);
    stream_recon_gz(file, &ParseOptions::default(), |row| est.observe(row)).unwrap();

    let report = est.finish();
    assert!(report.findings.is_empty(), "rates must not be applied before they exist");
    assert_eq!(report.total_monthly_run_rate, Decimal::ZERO);
}
