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

use std::cell::Cell;
use std::collections::HashSet;
use std::io::Read;
use std::rc::Rc;

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

/// Where an in-flight analysis has got to.
///
/// Four phases because four things actually happen, not to pad a progress bar: bytes are
/// pulled off the decoder, rows are parsed and classified in one pass, the detector rolls
/// lines up per subscription and applies the noise gate, then the payload is built and
/// ordered. `PARSING` is where all the time goes on a large file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Phase {
    /// Bytes are being pulled in; no row has been handed over yet.
    Reading,
    /// Streaming rows through the parser and the detector.
    Parsing,
    /// `EstDetector::finish` — per-subscription rollup and the noise gate.
    EstDetection,
    /// Building, sorting and serialising the payload.
    Finalizing,
}

impl Phase {
    /// The wire name, matching the `Phase` union in `apps/web/src/lib/types.ts`.
    pub fn as_str(self) -> &'static str {
        match self {
            Phase::Reading => "READING",
            Phase::Parsing => "PARSING",
            Phase::EstDetection => "EST_DETECTION",
            Phase::Finalizing => "FINALIZING",
        }
    }
}

/// One progress tick.
///
/// Every field is a fact already known, never an extrapolation: `bytes_processed` is what
/// the decoder has actually consumed of the *source* bytes, so it is comparable with the
/// file size the user sees on disk, and it is what the percentage should be computed from.
/// Row counts cannot be turned into a percentage here — the total is unknowable until the
/// last row is read — so the UI estimates that itself, and says that it is an estimate.
/// Field names cross as camelCase, unlike [`AnalysisResult`]: this object is consumed by
/// hand-written UI code rather than mirrored field-for-field from Rust, and `bytesProcessed`
/// is what reads naturally there.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub bytes_processed: u64,
    pub total_bytes: u64,
    pub rows_parsed: u64,
    /// EST lines matched so far. Pre-rollup and pre-noise-gate, so it can exceed the
    /// finding count in the finished report. See [`EstDetector::finding_count`].
    pub est_found: u64,
    pub phase: Phase,
}

/// Counts bytes handed out of a slice.
///
/// Wrapping the *source* rather than counting decompressed output is deliberate: for a
/// `.csv.gz` the user's sense of "how far through" is the compressed size, which is what
/// the file manager showed them. The reader downstream is buffered, so the count advances
/// in 64 KiB steps and leads the row callback by at most one buffer.
struct CountingReader<'a> {
    inner: &'a [u8],
    consumed: Rc<Cell<u64>>,
}

impl Read for CountingReader<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.consumed.set(self.consumed.get() + n as u64);
        Ok(n)
    }
}

/// Parse and analyse a reconciliation export.
///
/// Accepts gzip (including multi-member) or plain CSV; the format is decided from the
/// magic bytes, not the file name. Returns a human-readable message on failure — it goes
/// straight into the UI.
pub fn analyze(bytes: &[u8]) -> Result<AnalysisResult, String> {
    analyze_with_progress(bytes, 0, |_| {})
}

/// [`analyze`], reporting progress as it goes.
///
/// `on_progress` fires at most once every `row_interval` rows, plus once on entry to each
/// phase and once at the end. `row_interval` of 0 means phase transitions only.
///
/// The callback is invoked from inside the parse loop, so it must be cheap. Anything that
/// can wait — a `postMessage`, a DOM write — should be throttled by the caller on top of
/// this; the interval here bounds how often the question is asked, not how expensive the
/// answer is.
pub fn analyze_with_progress<F>(
    bytes: &[u8],
    row_interval: u64,
    mut on_progress: F,
) -> Result<AnalysisResult, String>
where
    F: FnMut(&Progress),
{
    if bytes.is_empty() {
        return Err("That file is empty.".into());
    }

    let total_bytes = bytes.len() as u64;
    let consumed = Rc::new(Cell::new(0u64));
    let mut est = EstDetector::new();
    let mut currency = String::new();
    let mut mixed_currency = false;

    let mut tick = |phase: Phase, rows: u64, est_found: usize, consumed: u64| {
        on_progress(&Progress {
            // Never report more than the file holds: the final buffered read can run past
            // the last row, and a bar that shows 101% undermines every other number here.
            bytes_processed: consumed.min(total_bytes),
            total_bytes,
            rows_parsed: rows,
            est_found: est_found as u64,
            phase,
        });
    };

    tick(Phase::Reading, 0, 0, 0);

    // Next row count at which to report. Rows are counted here rather than read back from
    // `StreamStats`, which only exists once the stream has finished.
    let mut rows_seen = 0u64;
    let mut next_report = row_interval;
    let reader = CountingReader { inner: bytes, consumed: Rc::clone(&consumed) };

    let options = ParseOptions::default();
    let stats = stream_recon_auto(reader, &options, |row: &ReconRow| {
        if !row.currency.is_empty() {
            if currency.is_empty() {
                currency = row.currency.clone();
            } else if currency != row.currency {
                mixed_currency = true;
            }
        }
        est.observe(row);

        rows_seen += 1;
        if row_interval > 0 && rows_seen >= next_report {
            next_report = rows_seen + row_interval;
            tick(Phase::Parsing, rows_seen, est.finding_count(), consumed.get());
        }
    })
    .map_err(|e| format!("Could not read that file: {e}"))?;

    tick(Phase::EstDetection, stats.rows_parsed, est.finding_count(), consumed.get());
    let report = est.finish();

    tick(Phase::Finalizing, stats.rows_parsed, report.findings.len(), consumed.get());
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

    /// A CSV with `rows` data rows, all of them EST +23% lines worth well over the gate.
    fn est_csv(rows: usize) -> String {
        let mut s = String::from(
            "CustomerId,SubscriptionId,ChargeType,EffectiveUnitPrice,UnitPrice,\
             BillableQuantity,Currency,ChargeStartDate,ChargeEndDate,TermAndBillingCycle\r\n",
        );
        for i in 0..rows {
            s.push_str(&format!(
                "C{i},S{i},cycleCharge,'12.30,'10.00,'20,USD,2026-07-01,2026-07-31,\
                 \"Monthly term, Monthly billing\"\r\n"
            ));
        }
        s
    }

    fn collect_progress(bytes: &[u8], interval: u64) -> (Vec<Progress>, AnalysisResult) {
        let mut seen = Vec::new();
        let result = analyze_with_progress(bytes, interval, |p| seen.push(*p)).unwrap();
        (seen, result)
    }

    #[test]
    fn progress_reports_every_phase_even_for_a_tiny_file() {
        // 14 rows with a 25k interval: the row trigger never fires, so this is the phase
        // transitions alone. A file too small to tick must still reach a terminal state.
        let (seen, _) = collect_progress(FIXTURE, 25_000);
        let phases: Vec<Phase> = seen.iter().map(|p| p.phase).collect();
        assert_eq!(phases, vec![Phase::Reading, Phase::EstDetection, Phase::Finalizing]);

        let last = seen.last().unwrap();
        assert_eq!(last.rows_parsed, 14);
        assert_eq!(last.bytes_processed, last.total_bytes, "must finish at 100%");
    }

    #[test]
    fn progress_ticks_while_parsing_and_never_goes_backwards() {
        let csv = est_csv(1_000);
        let (seen, result) = collect_progress(csv.as_bytes(), 100);

        let parsing: Vec<&Progress> = seen.iter().filter(|p| p.phase == Phase::Parsing).collect();
        assert_eq!(parsing.len(), 10, "1000 rows at one tick per 100");

        for pair in seen.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            assert!(b.rows_parsed >= a.rows_parsed, "rows went backwards: {a:?} -> {b:?}");
            assert!(b.bytes_processed >= a.bytes_processed, "bytes went backwards");
            assert!(b.est_found >= a.est_found, "est count went backwards");
        }

        // The tally the user watched has to land on the number the report states.
        assert_eq!(seen.last().unwrap().rows_parsed, result.rows_parsed);
        assert_eq!(seen.last().unwrap().est_found as usize, result.findings.len());
    }

    #[test]
    fn bytes_processed_is_bounded_by_the_file_size() {
        // The buffered reader pulls 64 KiB at a time and will have swallowed the whole
        // file long before the last row is handed over. Reporting >100% is not an option.
        let csv = est_csv(500);
        let (seen, _) = collect_progress(csv.as_bytes(), 10);
        let total = csv.len() as u64;
        for p in &seen {
            assert_eq!(p.total_bytes, total);
            assert!(p.bytes_processed <= total, "{} > {total}", p.bytes_processed);
        }
        assert_eq!(seen.last().unwrap().bytes_processed, total);
    }

    #[test]
    fn a_zero_interval_asks_only_at_phase_boundaries() {
        let csv = est_csv(500);
        let (seen, _) = collect_progress(csv.as_bytes(), 0);
        assert!(seen.iter().all(|p| p.phase != Phase::Parsing));
        assert_eq!(seen.len(), 3);
    }

    #[test]
    fn progress_does_not_change_the_result() {
        let csv = est_csv(50);
        let quiet = analyze(csv.as_bytes()).unwrap();
        let (_, loud) = collect_progress(csv.as_bytes(), 1);
        assert_eq!(quiet.total_est_leak_monthly.value, loud.total_est_leak_monthly.value);
        assert_eq!(quiet.rows_parsed, loud.rows_parsed);
        assert_eq!(quiet.findings.len(), loud.findings.len());
    }

    #[test]
    fn the_progress_object_keys_are_the_ones_the_ui_reads() {
        let json = serde_json::to_value(Progress {
            bytes_processed: 1,
            total_bytes: 2,
            rows_parsed: 3,
            est_found: 4,
            phase: Phase::Parsing,
        })
        .unwrap();
        let mut keys: Vec<&str> = json.as_object().unwrap().keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(keys, ["bytesProcessed", "estFound", "phase", "rowsParsed", "totalBytes"]);
    }

    #[test]
    fn phase_names_match_the_typescript_union() {
        // These strings are the wire contract with apps/web/src/lib/types.ts.
        let names: Vec<String> = [Phase::Reading, Phase::Parsing, Phase::EstDetection, Phase::Finalizing]
            .iter()
            .map(|p| serde_json::to_string(p).unwrap())
            .collect();
        assert_eq!(names, vec!["\"READING\"", "\"PARSING\"", "\"EST_DETECTION\"", "\"FINALIZING\""]);
        assert_eq!(Phase::EstDetection.as_str(), "EST_DETECTION");
    }

    #[test]
    fn failures_come_back_as_sentences_not_debug_output() {
        assert_eq!(analyze(b"").unwrap_err(), "That file is empty.");

        let err = analyze(b"NotEvenClose,Header\r\n1,2\r\n").unwrap_err();
        assert!(err.starts_with("Could not read that file:"), "{err}");
        assert!(err.contains("EffectiveUnitPrice"), "{err}");
    }
}
