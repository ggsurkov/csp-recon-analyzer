//! The JSON payload handed to JavaScript.
//!
//! Kept free of every `wasm-bindgen` type on purpose: this module compiles and unit-tests
//! on the host, so the shape of the contract with the UI is verified by `cargo test`
//! rather than by loading a page and squinting at it.
//!
//! # Money does not cross this boundary as a number
//!
//! A JS `number` is an IEEE-754 double. Serialising a [`Decimal`] into one would throw away
//! the exact arithmetic the whole crate exists to preserve, at the very last step, on the
//! figure the user actually reads. Every monetary value crosses as a [`Money`] — an exact
//! decimal string plus a preformatted display string — and the UI does no arithmetic at
//! all. Ordering is decided here, in Rust, so the front end never needs to.

use std::collections::HashSet;

use recon_core::detectors::est::{EstDetector, EstEvidenceKind, EstFinding, EstReport};
use recon_core::{stream_recon_auto, ParseOptions, ReconRow, RemediationWindow, StreamStats};
use rust_decimal::Decimal;
use serde::Serialize;

/// Version of the analyser that produced a result.
///
/// Travels with the payload rather than being a second exported function: a figure about
/// somebody's invoice is worth nothing without knowing which build of the rules produced
/// it, and coupling the two means they cannot drift apart.
pub const ANALYZER_VERSION: &str = env!("CARGO_PKG_VERSION");

/// A monetary amount, safe to hand to JavaScript.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Money {
    /// Exact value as a decimal string. Parse it with a decimal library, never `Number()`.
    pub value: String,
    /// Rounded to two places and formatted with the currency. For display only.
    pub display: String,
}

impl Money {
    pub fn new(value: Decimal, currency: &str) -> Self {
        // `round_dp` only ever removes places, so 88.8 stays "88.8"; the `{:.2}` pads it
        // out to a money-shaped "88.80".
        let magnitude = value.round_dp(2).abs();
        let sign = if value.is_sign_negative() && !magnitude.is_zero() { "-" } else { "" };
        let display = match symbol_for(currency) {
            Some(sym) => format!("{sign}{sym}{magnitude:.2}"),
            None if currency.is_empty() => format!("{sign}{magnitude:.2}"),
            None => format!("{sign}{magnitude:.2} {currency}"),
        };
        // Summing decimals accumulates scale (0.12 x 50 summed three times gives
        // "88.8000000000000000000000"). `normalize` strips the trailing zeros without
        // changing the value, so the exact string stays exact and stays readable.
        Money { value: value.normalize().to_string(), display }
    }
}

fn symbol_for(currency: &str) -> Option<&'static str> {
    match currency {
        "USD" | "CAD" | "AUD" | "NZD" => Some("$"),
        "EUR" => Some("\u{20ac}"),
        "GBP" => Some("\u{a3}"),
        _ => None,
    }
}

/// One EST line, shaped for the findings table.
#[derive(Debug, Clone, Serialize)]
pub struct FindingView {
    /// Row index in the source file. The evidence anchor the user can go and check.
    pub record_index: u64,

    pub customer_id: String,
    pub customer_name: String,
    pub subscription_id: String,
    pub sku_name: String,
    pub product_id: String,
    pub currency: String,

    pub charge_start_date: Option<String>,
    pub charge_end_date: Option<String>,

    /// `"23"` — the uplift as whole percent.
    pub rate_percent: String,
    /// `"+23%"` — ready to render.
    pub rate_label: String,
    /// Why that rate applies.
    pub reason: String,

    pub base_unit_price: Money,
    pub effective_unit_price: Money,
    pub uplift_per_seat: Money,
    /// Seats, as an exact string. Negative on a credit line.
    pub billable_quantity: String,
    pub uplift_amount: Money,
    /// The headline per-line figure.
    pub monthly_run_rate: Money,

    /// `"1.00"` / `"0.90"` / `"0.75"`.
    pub confidence: String,
    /// Plain-language version of the above.
    pub confidence_label: String,
    /// Which signal fired: `declared_and_priced` | `declared` | `price_ratio`.
    pub evidence_kind: String,
    /// `PriceAdjustmentDescription` verbatim; empty when detection was ratio-only.
    pub price_adjustment_description: String,

    /// `NOW` | `AT_RENEWAL` | `NEVER`.
    pub remediation_window: String,
    /// What the partner can actually do about it, in a sentence.
    pub action: String,

    /// True when this line's subscription total sits under the noise gate. Render it, but
    /// grey it out — it is excluded from the headline, not deleted.
    pub suppressed: bool,
}

/// Everything one analysis produced.
#[derive(Debug, Clone, Serialize)]
pub struct AnalysisResult {
    /// Headline EST penalty per month, above-threshold subscriptions only.
    pub total_est_leak_monthly: Money,
    /// Monthly total held back by the noise gate. Shown as a footnote, never dropped.
    pub suppressed_monthly: Money,
    pub suppressed_subscription_count: usize,
    /// The gate that was applied, so the UI can name the number.
    pub noise_threshold_monthly: Money,
    /// Billing currency of the export.
    pub currency: String,

    /// Every EST line found, ordered by monthly cost, suppressed ones last.
    pub findings: Vec<FindingView>,
    /// Distinct subscriptions behind the headline figure.
    ///
    /// Excludes suppressed ones, so "$88.80 across N subscriptions" stays arithmetically
    /// true. Suppressed subscriptions are counted separately, above.
    pub reportable_subscription_count: usize,

    pub rows_parsed: u64,
    pub rows_failed: u64,
    pub row_error_count: u64,
    /// Columns in the file this build does not model. Not an error; worth surfacing.
    pub unknown_columns: Vec<String>,
    /// Anything the user should know before trusting the number above.
    pub warnings: Vec<String>,

    /// Wall time spent inside WebAssembly, in milliseconds. Set by the binding layer.
    pub execution_time_ms: f64,
    /// Build of the analyser these figures came from. See [`ANALYZER_VERSION`].
    pub analyzer_version: String,
}

/// Parse and analyse a reconciliation export.
///
/// Accepts gzip (including multi-member) or plain CSV; the format is decided from the
/// magic bytes, not the file name. Returns a human-readable message on failure — it goes
/// straight into the UI.
pub fn analyze(bytes: &[u8]) -> Result<AnalysisResult, String> {
    if bytes.is_empty() {
        return Err("That file is empty.".into());
    }

    let mut est = EstDetector::new();
    let mut currency = String::new();
    let mut mixed_currency = false;

    let options = ParseOptions::default();
    let stats = stream_recon_auto(bytes, &options, |row: &ReconRow| {
        if !row.currency.is_empty() {
            if currency.is_empty() {
                currency = row.currency.clone();
            } else if currency != row.currency {
                mixed_currency = true;
            }
        }
        est.observe(row);
    })
    .map_err(|e| format!("Could not read that file: {e}"))?;

    let report = est.finish();
    Ok(build(report, stats, currency, mixed_currency))
}

fn build(
    report: EstReport,
    stats: StreamStats,
    currency: String,
    mixed_currency: bool,
) -> AnalysisResult {
    // The noise gate is decided per subscription, so a line inherits its subscription's
    // verdict. A set, not a list: this is probed once per finding, and a file where most
    // subscriptions fall under the gate would otherwise be quadratic.
    let suppressed_subscriptions: HashSet<(&str, &str)> = report
        .subscriptions
        .iter()
        .filter(|s| s.below_noise_threshold)
        .map(|s| (s.customer_id.as_str(), s.subscription_id.as_str()))
        .collect();

    // Order is settled here, on `Decimal`, so the UI never has to compare money. Sorting
    // the serialised strings instead would put "99.999" above "100.00".
    let mut ordered: Vec<(Decimal, FindingView)> = report
        .findings
        .iter()
        .map(|f| {
            let suppressed = suppressed_subscriptions
                .contains(&(f.customer_id.as_str(), f.subscription_id.as_str()));
            (f.monthly_run_rate, view(f, &currency, suppressed))
        })
        .collect();

    // Suppressed rows sink to the bottom; everything else by what it costs.
    ordered.sort_by(|(a_rate, a), (b_rate, b)| {
        a.suppressed
            .cmp(&b.suppressed)
            .then_with(|| b_rate.cmp(a_rate))
            .then_with(|| a.record_index.cmp(&b.record_index))
    });
    let findings: Vec<FindingView> = ordered.into_iter().map(|(_, v)| v).collect();

    let mut warnings = Vec::new();
    if mixed_currency {
        warnings.push(
            "This export mixes billing currencies. Totals are sums of unconverted amounts \
             and should not be read as a single figure."
                .into(),
        );
    }
    if stats.rows_failed > 0 {
        warnings.push(format!(
            "{} row(s) could not be read at all and are excluded from every figure here.",
            stats.rows_failed
        ));
    }
    if stats.row_error_count > 0 {
        warnings.push(format!(
            "{} cell(s) did not parse. Those fields were treated as absent, so the total below \
             may understate the leak.",
            stats.row_error_count
        ));
    }
    if !stats.unknown_columns.is_empty() {
        warnings.push(format!(
            "This file has {} column(s) this build does not model ({}). Nothing was dropped, \
             but a newer schema may carry findings we cannot see yet.",
            stats.unknown_columns.len(),
            stats.unknown_columns.join(", ")
        ));
    }
    if !stats.missing_columns.is_empty() {
        warnings.push(format!(
            "{} optional column(s) are absent from this export. Detection still ran, with less \
             corroboration than usual.",
            stats.missing_columns.len()
        ));
    }

    let reportable_subscription_count = report.reportable().count();

    AnalysisResult {
        total_est_leak_monthly: Money::new(report.total_monthly_run_rate, &currency),
        suppressed_monthly: Money::new(report.suppressed_monthly_run_rate, &currency),
        suppressed_subscription_count: report.suppressed_subscription_count,
        noise_threshold_monthly: Money::new(report.noise_threshold_monthly, &currency),
        currency,
        reportable_subscription_count,
        findings,
        rows_parsed: stats.rows_parsed,
        rows_failed: stats.rows_failed,
        row_error_count: stats.row_error_count,
        unknown_columns: stats.unknown_columns,
        warnings,
        execution_time_ms: 0.0,
        analyzer_version: ANALYZER_VERSION.to_owned(),
    }
}

fn view(f: &EstFinding, currency: &str, suppressed: bool) -> FindingView {
    let line_currency = if f.currency.is_empty() { currency } else { f.currency.as_str() };
    let percent = (f.rate * Decimal::ONE_HUNDRED).normalize();

    FindingView {
        record_index: f.record_index,
        customer_id: f.customer_id.clone(),
        customer_name: f.customer_name.clone(),
        subscription_id: f.subscription_id.clone(),
        sku_name: f.sku_name.clone(),
        product_id: f.product_id.clone(),
        currency: line_currency.to_owned(),
        charge_start_date: f.charge_start_date.map(|d| d.to_string()),
        charge_end_date: f.charge_end_date.map(|d| d.to_string()),
        rate_percent: percent.to_string(),
        rate_label: format!("+{percent}%"),
        reason: f.reason.to_owned(),
        base_unit_price: Money::new(f.base_unit_price, line_currency),
        effective_unit_price: Money::new(f.effective_unit_price, line_currency),
        uplift_per_seat: Money::new(f.uplift_per_seat, line_currency),
        billable_quantity: f.billable_quantity.normalize().to_string(),
        uplift_amount: Money::new(f.uplift_amount, line_currency),
        monthly_run_rate: Money::new(f.monthly_run_rate, line_currency),
        confidence: f.confidence.round_dp(2).to_string(),
        confidence_label: confidence_label(f.evidence_kind).to_owned(),
        evidence_kind: f.evidence_kind.as_str().to_owned(),
        price_adjustment_description: f.price_adjustment_description.clone(),
        remediation_window: f.remediation_window.as_str().to_owned(),
        action: action_for(f.remediation_window).to_owned(),
        suppressed,
    }
}

fn confidence_label(kind: EstEvidenceKind) -> &'static str {
    match kind {
        EstEvidenceKind::DeclaredAndPriced => {
            "Confirmed \u{2014} stated on the line and the price agrees"
        }
        EstEvidenceKind::Declared => "Stated by Microsoft, price not corroborated",
        EstEvidenceKind::PriceRatio => {
            "Inferred from the price ratio \u{2014} verify before acting"
        }
    }
}

/// The sentence under the Action column.
///
/// EST rolls a subscription onto a monthly term, which is what makes it fixable now. When
/// the term does not read as monthly we say `AT_RENEWAL` instead of promising something the
/// partner may not be able to execute.
fn action_for(window: RemediationWindow) -> &'static str {
    match window {
        RemediationWindow::Now => {
            "Cancel or move onto a committed term. The penalty stops at the next monthly cycle."
        }
        RemediationWindow::AtRenewal => {
            "Committed until the term ends. Schedule the change now so it lands at renewal."
        }
        RemediationWindow::Never => "No action available. Informational only.",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    const FIXTURE: &[u8] = include_bytes!("../../../fixtures/mock_recon_2026.csv.gz");

    #[test]
    fn money_never_becomes_a_float() {
        let m = Money::new(dec!(88.8), "USD");
        assert_eq!(m.value, "88.8");
        assert_eq!(m.display, "$88.80");

        // Eighteen decimals survive the trip; only the display copy is rounded.
        let exact = Money::new(dec!(-28.387096774193548387), "USD");
        assert_eq!(exact.value, "-28.387096774193548387");
        assert_eq!(exact.display, "-$28.39");

        assert_eq!(Money::new(dec!(5), "PLN").display, "5.00 PLN");
        assert_eq!(Money::new(dec!(5), "").display, "5.00");
        assert_eq!(Money::new(Decimal::ZERO, "USD").display, "$0.00");
    }

    #[test]
    fn analyses_the_fixture_end_to_end() {
        let r = analyze(FIXTURE).expect("fixture analyses");

        assert_eq!(r.rows_parsed, 14, "both gzip members must be read");
        assert_eq!(r.rows_failed, 0);
        assert_eq!(r.currency, "USD");

        // Exact value is canonical, not zero-padded; the display copy is the money-shaped one.
        assert_eq!(r.total_est_leak_monthly.value, "88.8");
        assert_eq!(r.total_est_leak_monthly.display, "$88.80");
        assert_eq!(r.suppressed_monthly.value, "0.06");
        assert_eq!(r.suppressed_subscription_count, 1);
        assert_eq!(r.noise_threshold_monthly.display, "$5.00");

        assert_eq!(r.findings.len(), 4);
        // Four subscriptions carry a finding, but only three are behind the $88.80
        // headline — the fourth is under the gate and counted separately.
        assert_eq!(r.reportable_subscription_count, 3);

        // The build that produced these figures rides along with them.
        assert_eq!(r.analyzer_version, env!("CARGO_PKG_VERSION"));
        assert!(!r.analyzer_version.is_empty());

        // The unknown 49th column is surfaced rather than swallowed.
        assert_eq!(r.unknown_columns, vec!["NewMicrosoftColumn2026"]);
        assert_eq!(r.warnings.len(), 1, "{:?}", r.warnings);
        assert!(r.warnings[0].contains("NewMicrosoftColumn2026"));
    }

    #[test]
    fn findings_are_ordered_for_the_table() {
        let r = analyze(FIXTURE).unwrap();
        let order: Vec<&str> =
            r.findings.iter().map(|f| f.monthly_run_rate.display.as_str()).collect();
        // Costliest first; the sub-threshold line is last regardless of value.
        assert_eq!(order, vec!["$69.00", "$13.80", "$6.00", "$0.06"]);
        assert_eq!(
            r.findings.iter().map(|f| f.suppressed).collect::<Vec<_>>(),
            vec![false, false, false, true]
        );
    }

    #[test]
    fn the_twenty_three_percent_line_renders_completely() {
        let r = analyze(FIXTURE).unwrap();
        let f = &r.findings[0];

        assert_eq!(f.customer_name, "Fabrikam, Inc.");
        assert_eq!(f.sku_name, "Project Plan 3");
        assert_eq!(f.rate_label, "+23%");
        assert_eq!(f.effective_unit_price.display, "$36.90");
        assert_eq!(f.base_unit_price.display, "$30.00");
        assert_eq!(f.uplift_per_seat.display, "$6.90");
        assert_eq!(f.billable_quantity, "10");
        assert_eq!(f.monthly_run_rate.display, "$69.00");
        assert_eq!(f.confidence, "1.00");
        assert_eq!(f.evidence_kind, "declared_and_priced");
        assert_eq!(f.remediation_window, "NOW");
        assert!(f.action.starts_with("Cancel or move onto a committed term"));
        // Every row in the table can be traced back to a line in the file.
        assert_eq!(f.record_index, 2);
    }

    #[test]
    fn a_ratio_only_finding_says_it_is_inferred() {
        let r = analyze(FIXTURE).unwrap();
        let f = r.findings.iter().find(|f| f.subscription_id.ends_with("0010")).unwrap();
        assert_eq!(f.confidence, "0.75");
        assert!(f.confidence_label.contains("Inferred"));
        assert!(f.price_adjustment_description.is_empty());
    }

    #[test]
    fn plain_csv_is_accepted_too() {
        let csv = "CustomerId,SubscriptionId,ChargeType,EffectiveUnitPrice,UnitPrice,\
                   BillableQuantity,Currency,ChargeStartDate,ChargeEndDate,TermAndBillingCycle\r\n\
                   C1,S1,cycleCharge,'12.30,'10.00,'20,USD,2026-07-01,2026-07-31,\
                   \"Monthly term, Monthly billing\"\r\n";
        let r = analyze(csv.as_bytes()).unwrap();
        assert_eq!(r.rows_parsed, 1);
        // 2.30 * 20 = 46.00
        assert_eq!(r.total_est_leak_monthly.display, "$46.00");
        assert_eq!(r.findings[0].rate_label, "+23%");
    }

    #[test]
    fn failures_come_back_as_sentences_not_debug_output() {
        assert_eq!(analyze(b"").unwrap_err(), "That file is empty.");

        let err = analyze(b"NotEvenClose,Header\r\n1,2\r\n").unwrap_err();
        assert!(err.starts_with("Could not read that file:"), "{err}");
        assert!(err.contains("EffectiveUnitPrice"), "{err}");
    }
}
