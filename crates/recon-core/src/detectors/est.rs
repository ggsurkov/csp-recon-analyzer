//! `EST_UPLIFT` — Extended Service Terms penalty detection.
//!
//! # What changed
//!
//! Until 4 May 2026 a CSP subscription that was neither renewed nor cancelled sat in a free
//! 30-day grace period. Microsoft removed that. A subscription in the same state now rolls
//! onto an **Extended Service Term**: a monthly term charged at a premium over list.
//!
//! * **+3%** when the SKU has a monthly plan available.
//! * **+23%** when it does not.
//!
//! Nobody notices. The subscription keeps working, the seat count does not move, and the
//! invoice grows by a few percent on a line that looks identical to last month's. It is a
//! calendar-driven, entirely predictable leak, which is exactly why it is worth detecting.
//!
//! # How this detects it
//!
//! Two independent signals, because Microsoft does not always populate the prose field:
//!
//! 1. **Declared** — `PriceAdjustmentDescription` names Extended Service Terms. The rate is
//!    read out of the text when it is there.
//! 2. **PriceRatio** — `EffectiveUnitPrice / UnitPrice` lands on `1.03` or `1.23` within
//!    tolerance.
//!
//! Both firing is the strong case (`confidence 1.00`). Ratio alone is suggestive but not
//! proof — some other adjustment could coincidentally land on the same multiple — so it is
//! reported at `0.75` and labelled as such in the UI rather than being hidden.
//!
//! # Rates live in a table, not in the code
//!
//! Microsoft has changed the grace-period rules once and will change these percentages
//! eventually. [`EstPolicy::table`] is versioned by effective date so that reprocessing a
//! February file does not apply August's rules to it.

use std::collections::BTreeMap;

use chrono::NaiveDate;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use crate::numeric::first_percentage;
use crate::types::{ReconRow, RemediationWindow, TermDuration};

/// One uplift rate that EST can apply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EstRate {
    /// Fraction over list, e.g. `0.23`.
    pub rate: Decimal,
    /// Why this rate applies. Shown verbatim to the user.
    pub reason: &'static str,
}

/// Versioned Extended Service Terms policy.
///
/// Constructed from [`EstPolicy::for_date`] rather than by hand, so that every number in
/// here is tied to the date it came into force.
#[derive(Debug, Clone)]
pub struct EstPolicy {
    /// First charge date this policy applies to.
    pub effective_from: NaiveDate,
    /// Uplift rates in force.
    pub rates: Vec<EstRate>,
    /// How far `EffectiveUnitPrice / UnitPrice` may sit from `1 + rate` and still count.
    /// Absorbs Microsoft's own rounding, nothing more.
    pub ratio_tolerance: Decimal,
    /// Lowercased substrings that identify an EST line in `PriceAdjustmentDescription`.
    pub description_markers: &'static [&'static str],
    /// Findings whose monthly run rate is below this are kept but excluded from the
    /// headline total. Below it, FX drift and daily rounding produce more noise than signal.
    pub noise_threshold_monthly: Decimal,
    /// Days per month used to annualise non-monthly charge periods.
    pub avg_days_per_month: Decimal,
}

impl EstPolicy {
    /// The policy table, oldest first. Add a row; never edit one in place.
    pub fn table() -> Vec<EstPolicy> {
        vec![EstPolicy {
            // Grace period abolished, Extended Service Terms introduced.
            effective_from: NaiveDate::from_ymd_opt(2026, 5, 4).expect("valid date"),
            rates: vec![
                EstRate { rate: dec!(0.03), reason: "monthly plan available for this SKU" },
                EstRate { rate: dec!(0.23), reason: "no monthly plan exists for this SKU" },
            ],
            ratio_tolerance: dec!(0.0005),
            description_markers: &["extended service terms", "extended service term", "est fee"],
            noise_threshold_monthly: dec!(5),
            avg_days_per_month: dec!(30.436875),
        }]
    }

    /// The policy in force on `date`, or `None` if EST did not exist yet.
    pub fn for_date(date: NaiveDate) -> Option<EstPolicy> {
        EstPolicy::table().into_iter().rfind(|p| date >= p.effective_from)
    }

    /// The most recent policy. Used when a line carries no usable charge date.
    pub fn current() -> EstPolicy {
        EstPolicy::table().pop().expect("policy table is never empty")
    }

    fn rate_for(&self, value: Decimal) -> Option<EstRate> {
        self.rates.iter().copied().find(|r| (r.rate - value).abs() <= self.ratio_tolerance)
    }
}

/// Which signal fired.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EstEvidenceKind {
    /// `PriceAdjustmentDescription` said so, and the price ratio agrees.
    DeclaredAndPriced,
    /// `PriceAdjustmentDescription` said so; `UnitPrice` was missing or did not corroborate.
    Declared,
    /// Price ratio only. Suggestive, not proof.
    PriceRatio,
}

impl EstEvidenceKind {
    /// Confidence attached to a finding from this signal.
    pub fn confidence(self) -> Decimal {
        match self {
            EstEvidenceKind::DeclaredAndPriced => dec!(1.00),
            EstEvidenceKind::Declared => dec!(0.90),
            EstEvidenceKind::PriceRatio => dec!(0.75),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            EstEvidenceKind::DeclaredAndPriced => "declared_and_priced",
            EstEvidenceKind::Declared => "declared",
            EstEvidenceKind::PriceRatio => "price_ratio",
        }
    }
}

/// One reconciliation line carrying an EST penalty.
#[derive(Debug, Clone, PartialEq)]
pub struct EstFinding {
    /// Index of the source row. The evidence link.
    pub record_index: u64,

    pub customer_id: String,
    pub customer_name: String,
    pub subscription_id: String,
    pub product_id: String,
    pub sku_id: String,
    pub sku_name: String,
    pub currency: String,

    pub charge_start_date: Option<NaiveDate>,
    pub charge_end_date: Option<NaiveDate>,
    pub term_duration: TermDuration,

    /// The uplift fraction, e.g. `0.23`.
    pub rate: Decimal,
    /// Why that rate applies.
    pub reason: &'static str,
    /// List price the uplift was applied to.
    pub base_unit_price: Decimal,
    /// What was actually charged.
    pub effective_unit_price: Decimal,
    /// `effective - base`.
    pub uplift_per_seat: Decimal,
    /// Signed seats on the line — negative on a credit, so credits net out.
    pub billable_quantity: Decimal,
    /// Penalty for this line's charge period, in the export's currency.
    pub uplift_amount: Decimal,
    /// The same penalty normalised to a month. This is the number an MSP acts on.
    pub monthly_run_rate: Decimal,

    pub evidence_kind: EstEvidenceKind,
    pub confidence: Decimal,
    pub remediation_window: RemediationWindow,
    /// `PriceAdjustmentDescription` verbatim, empty when detection was ratio-only.
    pub price_adjustment_description: String,
}

/// Findings for one subscription, collapsed.
///
/// One alert per (customer, subscription) with a running total — not one per line. A
/// subscription with a cycle charge plus two prorations is one leak, and mailing three
/// alerts about it is how notifications get switched off.
#[derive(Debug, Clone, PartialEq)]
pub struct EstSubscriptionSummary {
    pub customer_id: String,
    pub customer_name: String,
    pub subscription_id: String,
    pub sku_name: String,
    pub currency: String,
    pub rate: Decimal,
    pub reason: &'static str,
    /// Sum over the lines in this file.
    pub uplift_amount: Decimal,
    /// Sum of the per-line monthly run rates.
    pub monthly_run_rate: Decimal,
    pub line_count: usize,
    /// Lowest confidence among the contributing lines.
    pub confidence: Decimal,
    pub remediation_window: RemediationWindow,
    /// Source row indices, for the evidence drawer.
    pub record_indices: Vec<u64>,
    /// True when this subscription's total sits under the noise threshold and is therefore
    /// excluded from the headline figures.
    pub below_noise_threshold: bool,
}

/// Everything the EST detector concluded about one file.
#[derive(Debug, Clone)]
pub struct EstReport {
    /// Every line that matched, including ones later suppressed as noise.
    pub findings: Vec<EstFinding>,
    /// Per-subscription rollups, largest monthly run rate first.
    pub subscriptions: Vec<EstSubscriptionSummary>,
    /// Headline monthly figure: above-threshold subscriptions only.
    pub total_monthly_run_rate: Decimal,
    /// Penalty billed in this file, above-threshold subscriptions only.
    pub total_uplift_amount: Decimal,
    /// Monthly total hidden by the noise gate. Shown as a footnote, never silently dropped.
    pub suppressed_monthly_run_rate: Decimal,
    pub suppressed_subscription_count: usize,
    /// The threshold that was applied, so the UI can name it.
    pub noise_threshold_monthly: Decimal,
}

impl EstReport {
    /// Subscriptions that clear the noise gate.
    pub fn reportable(&self) -> impl Iterator<Item = &EstSubscriptionSummary> {
        self.subscriptions.iter().filter(|s| !s.below_noise_threshold)
    }
}

/// Accumulates EST findings across a stream. Feed it every row; ask for the report at the end.
///
/// ```no_run
/// # use recon_core::{stream_recon_gz, ParseOptions, detectors::est::EstDetector};
/// let file = std::fs::File::open("recon.csv.gz").unwrap();
/// let mut est = EstDetector::new();
/// stream_recon_gz(file, &ParseOptions::default(), |row| est.observe(row)).unwrap();
/// let report = est.finish();
/// println!("EST is costing {} {}/month", report.total_monthly_run_rate, "USD");
/// ```
#[derive(Debug)]
pub struct EstDetector {
    policy: EstPolicy,
    findings: Vec<EstFinding>,
}

impl Default for EstDetector {
    fn default() -> Self {
        Self::new()
    }
}

impl EstDetector {
    /// Detector using the policy currently in force.
    pub fn new() -> Self {
        EstDetector { policy: EstPolicy::current(), findings: Vec::new() }
    }

    /// Detector pinned to a specific policy version — for reprocessing historical files.
    pub fn with_policy(policy: EstPolicy) -> Self {
        EstDetector { policy, findings: Vec::new() }
    }

    pub fn policy(&self) -> &EstPolicy {
        &self.policy
    }

    /// Lines matched so far, mid-stream.
    ///
    /// This is a *line* count, not the subscription count in the final report: the noise
    /// gate and the per-subscription rollup only happen in [`finish`](Self::finish). It
    /// exists so a caller streaming a large file can show progress. Treat it as a running
    /// tally that may shrink at `finish` once suppression is applied.
    pub fn finding_count(&self) -> usize {
        self.findings.len()
    }

    /// Examine one row. Cheap: no allocation unless the row matches.
    pub fn observe(&mut self, row: &ReconRow) {
        if let Some(f) = self.classify(row) {
            self.findings.push(f);
        }
    }

    fn classify(&self, row: &ReconRow) -> Option<EstFinding> {
        // A charge that predates the policy cannot be an EST charge, whatever the numbers
        // happen to look like. A missing date is not disqualifying — the file may be a
        // partial export — but a date we can read and that is too early is.
        if let Some(start) = row.charge_start_date {
            if start < self.policy.effective_from {
                return None;
            }
        }

        // Everything downstream is built on the price actually charged for the seats
        // actually billed. `UnitPrice` and `Quantity` are corroboration only.
        let effective = row.effective_unit_price?;
        if effective <= Decimal::ZERO {
            return None;
        }
        let quantity = row.signed_billable_quantity()?;
        if quantity.is_zero() {
            return None;
        }

        let description = row.price_adjustment_description.to_ascii_lowercase();
        let declared = self.policy.description_markers.iter().any(|m| description.contains(m));

        // Ratio signal: how far above list was this line actually charged?
        let ratio_rate = row
            .unit_price
            .filter(|list| *list > Decimal::ZERO)
            .map(|list| (effective - list) / list)
            .and_then(|excess| self.policy.rate_for(excess));

        let (rate_info, kind) = match (declared, ratio_rate) {
            (true, Some(r)) => (r, EstEvidenceKind::DeclaredAndPriced),
            // Declared but the price does not corroborate: trust the declaration and take
            // the rate from the prose, falling back to the smallest configured rate so we
            // under-claim rather than over-claim.
            (true, None) => {
                let from_text = first_percentage(&description)
                    .map(|p| p / dec!(100))
                    .and_then(|p| self.policy.rate_for(p));
                match from_text {
                    Some(r) => (r, EstEvidenceKind::Declared),
                    None => (*self.policy.rates.first()?, EstEvidenceKind::Declared),
                }
            }
            (false, Some(r)) => (r, EstEvidenceKind::PriceRatio),
            (false, None) => return None,
        };

        // Prefer the list price the file gives us; derive it only when we have to.
        let base_unit_price = match row.unit_price {
            Some(list) if list > Decimal::ZERO && kind != EstEvidenceKind::Declared => list,
            _ => effective / (Decimal::ONE + rate_info.rate),
        };
        let uplift_per_seat = effective - base_unit_price;
        if uplift_per_seat <= Decimal::ZERO {
            return None;
        }

        let uplift_amount = uplift_per_seat * quantity;

        Some(EstFinding {
            record_index: row.record_index,
            customer_id: row.customer_id.clone(),
            customer_name: row.customer_name.clone(),
            subscription_id: row.subscription_id.clone(),
            product_id: row.product_id.clone(),
            sku_id: row.sku_id.clone(),
            sku_name: row.sku_name.clone(),
            currency: row.currency.clone(),
            charge_start_date: row.charge_start_date,
            charge_end_date: row.charge_end_date,
            term_duration: row.term.duration,
            rate: rate_info.rate,
            reason: rate_info.reason,
            base_unit_price,
            effective_unit_price: effective,
            uplift_per_seat,
            billable_quantity: quantity,
            uplift_amount,
            monthly_run_rate: self.monthly_run_rate(row, uplift_amount),
            evidence_kind: kind,
            confidence: kind.confidence(),
            remediation_window: remediation_window(row),
            price_adjustment_description: row.price_adjustment_description.clone(),
        })
    }

    /// Normalise a period penalty to a monthly figure.
    ///
    /// A 28-31 day period *is* a month; rescaling it by an average month length would
    /// introduce an error where there is none. Anything else gets scaled by day count.
    fn monthly_run_rate(&self, row: &ReconRow, uplift_amount: Decimal) -> Decimal {
        match row.charge_days() {
            Some(28..=31) | None => uplift_amount,
            Some(d) if d > 0 => uplift_amount / Decimal::from(d) * self.policy.avg_days_per_month,
            Some(_) => uplift_amount,
        }
    }

    /// Roll the findings up and apply the noise gate.
    pub fn finish(self) -> EstReport {
        let mut grouped: BTreeMap<(String, String), EstSubscriptionSummary> = BTreeMap::new();

        for f in &self.findings {
            let key = (f.customer_id.clone(), f.subscription_id.clone());
            match grouped.get_mut(&key) {
                Some(s) => {
                    s.uplift_amount += f.uplift_amount;
                    s.monthly_run_rate += f.monthly_run_rate;
                    s.line_count += 1;
                    s.confidence = s.confidence.min(f.confidence);
                    s.record_indices.push(f.record_index);
                    // Mixed windows collapse to the more conservative one.
                    if f.remediation_window == RemediationWindow::AtRenewal {
                        s.remediation_window = RemediationWindow::AtRenewal;
                    }
                }
                None => {
                    grouped.insert(
                        key,
                        EstSubscriptionSummary {
                            customer_id: f.customer_id.clone(),
                            customer_name: f.customer_name.clone(),
                            subscription_id: f.subscription_id.clone(),
                            sku_name: f.sku_name.clone(),
                            currency: f.currency.clone(),
                            rate: f.rate,
                            reason: f.reason,
                            uplift_amount: f.uplift_amount,
                            monthly_run_rate: f.monthly_run_rate,
                            line_count: 1,
                            confidence: f.confidence,
                            remediation_window: f.remediation_window,
                            record_indices: vec![f.record_index],
                            below_noise_threshold: false,
                        },
                    );
                }
            }
        }

        let threshold = self.policy.noise_threshold_monthly;
        let mut subscriptions: Vec<EstSubscriptionSummary> = grouped.into_values().collect();

        let mut total_monthly = Decimal::ZERO;
        let mut total_uplift = Decimal::ZERO;
        let mut suppressed_monthly = Decimal::ZERO;
        let mut suppressed_count = 0usize;

        for s in &mut subscriptions {
            s.below_noise_threshold = s.monthly_run_rate.abs() < threshold;
            if s.below_noise_threshold {
                suppressed_monthly += s.monthly_run_rate;
                suppressed_count += 1;
            } else {
                total_monthly += s.monthly_run_rate;
                total_uplift += s.uplift_amount;
            }
        }

        subscriptions.sort_by(|a, b| {
            b.monthly_run_rate
                .cmp(&a.monthly_run_rate)
                .then_with(|| a.customer_id.cmp(&b.customer_id))
                .then_with(|| a.subscription_id.cmp(&b.subscription_id))
        });

        EstReport {
            findings: self.findings,
            subscriptions,
            total_monthly_run_rate: total_monthly,
            total_uplift_amount: total_uplift,
            suppressed_monthly_run_rate: suppressed_monthly,
            suppressed_subscription_count: suppressed_count,
            noise_threshold_monthly: threshold,
        }
    }
}

/// When an EST charge can be stopped.
///
/// EST puts the subscription on a **monthly** term, which is precisely what makes it
/// fixable today: cancel or renew onto a proper term and the penalty stops next cycle. If
/// the term does not read as monthly the file is telling us something we did not expect, so
/// we downgrade to `AT_RENEWAL` rather than promise an action that may not be available.
fn remediation_window(row: &ReconRow) -> RemediationWindow {
    match row.term.duration {
        TermDuration::Monthly => RemediationWindow::Now,
        _ => RemediationWindow::AtRenewal,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{BillingCycle, BillingTerm, ChargeType};

    fn row(effective: Decimal, list: Decimal, qty: Decimal, desc: &str) -> ReconRow {
        ReconRow {
            record_index: 0,
            customer_id: "C1".into(),
            subscription_id: "S1".into(),
            charge_type: ChargeType::CycleCharge,
            charge_start_date: NaiveDate::from_ymd_opt(2026, 7, 1),
            charge_end_date: NaiveDate::from_ymd_opt(2026, 7, 31),
            term: BillingTerm { duration: TermDuration::Monthly, cycle: BillingCycle::Monthly },
            unit_price: Some(list),
            effective_unit_price: Some(effective),
            billable_quantity: Some(qty),
            price_adjustment_description: desc.into(),
            ..Default::default()
        }
    }

    fn single(r: &ReconRow) -> Option<EstFinding> {
        let mut d = EstDetector::new();
        d.observe(r);
        d.findings.into_iter().next()
    }

    #[test]
    fn declared_and_priced_is_the_strong_case() {
        let f = single(&row(
            dec!(36.90),
            dec!(30.00),
            dec!(10),
            "Extended Service Terms 23% Fee Applied",
        ))
        .expect("finding");
        assert_eq!(f.rate, dec!(0.23));
        assert_eq!(f.evidence_kind, EstEvidenceKind::DeclaredAndPriced);
        assert_eq!(f.confidence, dec!(1.00));
        assert_eq!(f.uplift_amount, dec!(69.00));
        assert_eq!(f.monthly_run_rate, dec!(69.00));
        assert_eq!(f.remediation_window, RemediationWindow::Now);
    }

    #[test]
    fn price_ratio_alone_still_fires_at_lower_confidence() {
        let f = single(&row(dec!(18.45), dec!(15.00), dec!(4), "")).expect("finding");
        assert_eq!(f.rate, dec!(0.23));
        assert_eq!(f.evidence_kind, EstEvidenceKind::PriceRatio);
        assert_eq!(f.confidence, dec!(0.75));
        assert_eq!(f.uplift_amount, dec!(13.80));
    }

    #[test]
    fn declared_without_a_list_price_derives_the_base() {
        let mut r = row(dec!(36.90), dec!(0), dec!(10), "Extended Service Terms 23% Fee Applied");
        r.unit_price = None;
        let f = single(&r).expect("finding");
        assert_eq!(f.evidence_kind, EstEvidenceKind::Declared);
        assert_eq!(f.base_unit_price, dec!(30));
        assert_eq!(f.uplift_amount, dec!(69.00));
    }

    #[test]
    fn ordinary_lines_do_not_fire() {
        assert!(single(&row(dec!(22), dec!(22), dec!(25), "")).is_none());
        // An expired promo is a price jump, but not a 3%/23% one.
        assert!(single(&row(dec!(14), dec!(10), dec!(30), "")).is_none());
        // A discount is not an uplift.
        assert!(single(&row(dec!(10), dec!(14), dec!(30), "")).is_none());
    }

    #[test]
    fn charges_before_the_policy_date_are_out_of_scope() {
        let mut r = row(dec!(4.12), dec!(4.00), dec!(50), "Extended Service Terms 3% Fee Applied");
        r.charge_start_date = NaiveDate::from_ymd_opt(2026, 4, 1);
        r.charge_end_date = NaiveDate::from_ymd_opt(2026, 4, 30);
        assert!(single(&r).is_none());
    }

    #[test]
    fn credit_lines_net_against_the_penalty() {
        let mut charge = row(dec!(4.12), dec!(4.00), dec!(50), "Extended Service Terms 3% Fee");
        charge.record_index = 0;
        let mut credit = row(dec!(4.12), dec!(4.00), dec!(-50), "Extended Service Terms 3% Fee");
        credit.record_index = 1;
        credit.charge_type = ChargeType::RemoveQuantity;

        let mut d = EstDetector::new();
        d.observe(&charge);
        d.observe(&credit);
        let report = d.finish();

        assert_eq!(report.findings.len(), 2);
        assert_eq!(report.subscriptions.len(), 1);
        assert_eq!(report.subscriptions[0].monthly_run_rate, Decimal::ZERO);
        // Netting to zero puts it under the gate, which is the right answer: the partner
        // paid the penalty and got it back.
        assert!(report.subscriptions[0].below_noise_threshold);
        assert_eq!(report.total_monthly_run_rate, Decimal::ZERO);
    }

    #[test]
    fn the_noise_gate_suppresses_but_never_deletes() {
        let mut d = EstDetector::new();
        d.observe(&row(dec!(2.06), dec!(2.00), dec!(1), "Extended Service Terms 3% Fee Applied"));
        let report = d.finish();

        assert_eq!(report.findings.len(), 1, "the finding is still there");
        assert_eq!(report.findings[0].uplift_amount, dec!(0.06));
        assert_eq!(report.total_monthly_run_rate, Decimal::ZERO, "but not in the headline");
        assert_eq!(report.suppressed_subscription_count, 1);
        assert_eq!(report.suppressed_monthly_run_rate, dec!(0.06));
        assert_eq!(report.reportable().count(), 0);
    }

    #[test]
    fn a_partial_period_is_scaled_to_a_month() {
        let mut r = row(dec!(4.12), dec!(4.00), dec!(50), "Extended Service Terms 3% Fee");
        // Ten days only.
        r.charge_end_date = NaiveDate::from_ymd_opt(2026, 7, 10);
        let f = single(&r).expect("finding");
        assert_eq!(f.uplift_amount, dec!(6.00));
        // 6.00 / 10 days * 30.436875 days
        assert_eq!(f.monthly_run_rate.round_dp(4), dec!(18.2621));
    }

    #[test]
    fn the_policy_table_is_keyed_by_date() {
        assert!(EstPolicy::for_date(NaiveDate::from_ymd_opt(2026, 5, 3).unwrap()).is_none());
        assert!(EstPolicy::for_date(NaiveDate::from_ymd_opt(2026, 5, 4).unwrap()).is_some());
        assert_eq!(EstPolicy::current().rates.len(), 2);
    }
}
