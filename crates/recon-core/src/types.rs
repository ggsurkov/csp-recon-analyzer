//! The typed view of one reconciliation line.

use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// The 48 canonical columns of the license-based reconciliation export, in the order
/// Microsoft emits them. Position is documentation only — the parser matches on
/// normalised *names*, because the order has changed before and will change again.
pub const CANONICAL_COLUMNS: [&str; 48] = [
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
];

/// Indices into [`CANONICAL_COLUMNS`]. Used as the key of the header lookup table.
#[allow(dead_code)]
pub(crate) mod col {
    pub const PARTNER_ID: usize = 0;
    pub const PARTNER_NAME: usize = 1;
    pub const CUSTOMER_ID: usize = 2;
    pub const CUSTOMER_NAME: usize = 3;
    pub const CUSTOMER_DOMAIN_NAME: usize = 4;
    pub const CUSTOMER_COUNTRY: usize = 5;
    pub const INVOICE_NUMBER: usize = 6;
    pub const MPN_ID: usize = 7;
    pub const TIER2_MPN_ID: usize = 8;
    pub const ORDER_ID: usize = 9;
    pub const ORDER_DATE: usize = 10;
    pub const PRODUCT_ID: usize = 11;
    pub const SKU_ID: usize = 12;
    pub const AVAILABILITY_ID: usize = 13;
    pub const SKU_NAME: usize = 14;
    pub const PRODUCT_NAME: usize = 15;
    pub const PUBLISHER_NAME: usize = 16;
    pub const PUBLISHER_ID: usize = 17;
    pub const SUBSCRIPTION_DESCRIPTION: usize = 18;
    pub const SUBSCRIPTION_ID: usize = 19;
    pub const CHARGE_START_DATE: usize = 20;
    pub const CHARGE_END_DATE: usize = 21;
    pub const TERM_AND_BILLING_CYCLE: usize = 22;
    pub const EFFECTIVE_UNIT_PRICE: usize = 23;
    pub const UNIT_TYPE: usize = 24;
    pub const QUANTITY: usize = 25;
    pub const SUBTOTAL: usize = 26;
    pub const TAX_TOTAL: usize = 27;
    pub const TOTAL: usize = 28;
    pub const CURRENCY: usize = 29;
    pub const PRICE_ADJUSTMENT_DESCRIPTION: usize = 30;
    pub const PUBLISHER_DISCOUNT: usize = 31;
    pub const DISCOUNT_DETAILS: usize = 32;
    pub const CHARGE_TYPE: usize = 33;
    pub const UNIT_PRICE: usize = 34;
    pub const BILLABLE_QUANTITY: usize = 35;
    pub const PRICING_CURRENCY: usize = 36;
    pub const PC_TO_BC_EXCHANGE_RATE: usize = 37;
    pub const PC_TO_BC_EXCHANGE_RATE_DATE: usize = 38;
    pub const METER_DESCRIPTION: usize = 39;
    pub const RESERVATION_ORDER_ID: usize = 40;
    pub const CREDIT_REASON_CODE: usize = 41;
    pub const SUBSCRIPTION_START_DATE: usize = 42;
    pub const SUBSCRIPTION_END_DATE: usize = 43;
    pub const REFERENCE_ID: usize = 44;
    pub const PRODUCT_QUALIFIERS: usize = 45;
    pub const PROMOTION_ID: usize = 46;
    pub const PRODUCT_CATEGORY: usize = 47;
}

/// Columns without which no detector can say anything true. Their absence is a fatal
/// error, unlike every other kind of schema drift.
pub(crate) const REQUIRED_COLUMNS: [usize; 5] = [
    col::CUSTOMER_ID,
    col::SUBSCRIPTION_ID,
    col::CHARGE_TYPE,
    col::EFFECTIVE_UNIT_PRICE,
    col::BILLABLE_QUANTITY,
];

// ---------------------------------------------------------------------------------------
// ChargeType
// ---------------------------------------------------------------------------------------

/// What the line *is*. Matched case-insensitively: the same export has been observed
/// carrying `cycleCharge`, `cyclecharge` and `CycleCharge` in one file.
///
/// Anything unrecognised lands in [`ChargeType::Other`] with the original spelling intact.
/// Microsoft adds charge types; an unknown one must show up in the UI as itself, not be
/// silently folded into a bucket we made up.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ChargeType {
    /// Recurring charge for a billing cycle.
    CycleCharge,
    /// Initial purchase.
    PurchaseFee,
    /// Seats added mid-cycle (prorated).
    AddQuantity,
    /// Seats removed mid-cycle (prorated credit).
    RemoveQuantity,
    /// Subscription cancelled inside the cancellation window (credit).
    CancelImmediate,
    /// Explicit credit / refund line.
    Credit,
    /// A charge type this build does not model, kept verbatim.
    Other(String),
}

impl ChargeType {
    /// Case- and separator-insensitive parse.
    pub fn parse(raw: &str) -> Self {
        let raw = raw.trim();
        let norm: String = raw
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .map(|c| c.to_ascii_lowercase())
            .collect();
        match norm.as_str() {
            "cyclecharge" | "cyclefee" | "cycleinstantfee" => ChargeType::CycleCharge,
            "purchasefee" | "newfee" | "purchase" | "activationfee" => ChargeType::PurchaseFee,
            "addquantity" | "addquantityfee" | "proratefeesaddquantity" => ChargeType::AddQuantity,
            "removequantity" | "removequantityfee" | "proratefeesremovequantity" => {
                ChargeType::RemoveQuantity
            }
            "cancelimmediate" | "cancelinstant" | "cancelfee" | "cancellationfee"
            | "proratefeescancel" => ChargeType::CancelImmediate,
            "credit" | "creditmemo" | "refund" => ChargeType::Credit,
            _ => ChargeType::Other(raw.to_owned()),
        }
    }

    /// The direction this charge type moves money in: `-1` gives money back to the
    /// partner, `+1` takes it.
    ///
    /// This is the *expected* sign, not a multiplier — see
    /// [`ReconRow::signed_billable_quantity`] for why that distinction matters.
    pub fn sign(&self) -> i8 {
        match self {
            ChargeType::RemoveQuantity | ChargeType::CancelImmediate | ChargeType::Credit => -1,
            _ => 1,
        }
    }

    /// True when the line reduces what the partner owes.
    pub fn is_credit(&self) -> bool {
        self.sign() < 0
    }

    pub fn as_str(&self) -> &str {
        match self {
            ChargeType::CycleCharge => "cycleCharge",
            ChargeType::PurchaseFee => "purchaseFee",
            ChargeType::AddQuantity => "addQuantity",
            ChargeType::RemoveQuantity => "removeQuantity",
            ChargeType::CancelImmediate => "cancelImmediate",
            ChargeType::Credit => "credit",
            ChargeType::Other(s) => s,
        }
    }
}

impl std::fmt::Display for ChargeType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

// ---------------------------------------------------------------------------------------
// ReferenceId
// ---------------------------------------------------------------------------------------

/// `ReferenceId` changed shape mid-2026: it used to be a scalar, now it is a JSON object.
/// Both forms appear in the same account depending on when the charge originated, so both
/// have to be readable forever.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ReferenceId {
    /// Empty cell.
    Absent,
    /// Pre-June-2026 scalar, or a JSON-looking value that failed to parse (kept verbatim
    /// rather than thrown away).
    Legacy(String),
    /// Post-June-2026 object form: `{"osId":"...","id":"...","v":2}`.
    V2 { os_id: Option<String>, id: Option<String>, v: i64 },
}

#[derive(Deserialize)]
struct ReferenceIdV2Wire {
    #[serde(rename = "osId", default)]
    os_id: Option<String>,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    v: i64,
}

impl ReferenceId {
    /// Parse a `ReferenceId` cell. Returns `(value, json_failed)`; a failed JSON parse is
    /// downgraded to [`ReferenceId::Legacy`] and reported as a non-fatal row error.
    pub fn parse(raw: &str) -> (Self, bool) {
        let s = crate::numeric::unguard(raw);
        if s.is_empty() {
            return (ReferenceId::Absent, false);
        }
        if s.starts_with('{') {
            return match serde_json::from_str::<ReferenceIdV2Wire>(s) {
                Ok(w) => (ReferenceId::V2 { os_id: w.os_id, id: w.id, v: w.v }, false),
                Err(_) => (ReferenceId::Legacy(s.to_owned()), true),
            };
        }
        (ReferenceId::Legacy(s.to_owned()), false)
    }

    /// The identifier itself, whichever shape it arrived in.
    pub fn id(&self) -> Option<&str> {
        match self {
            ReferenceId::Absent => None,
            ReferenceId::Legacy(s) => Some(s),
            ReferenceId::V2 { id, .. } => id.as_deref(),
        }
    }

    pub fn is_absent(&self) -> bool {
        matches!(self, ReferenceId::Absent)
    }
}

// ---------------------------------------------------------------------------------------
// Term
// ---------------------------------------------------------------------------------------

/// Commitment length. Drives whether a finding is actionable today.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TermDuration {
    Monthly,
    Annual,
    Triennial,
    Unknown,
}

/// How often the partner is invoiced. Independent of the commitment length: "annual term,
/// monthly billing" is the most common NCE shape and the one most often misread.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BillingCycle {
    Monthly,
    Annual,
    Triennial,
    OneTime,
    Unknown,
}

/// Parsed `TermAndBillingCycle`, e.g. `"Annual term, Monthly billing"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BillingTerm {
    pub duration: TermDuration,
    pub cycle: BillingCycle,
}

impl BillingTerm {
    pub const UNKNOWN: BillingTerm =
        BillingTerm { duration: TermDuration::Unknown, cycle: BillingCycle::Unknown };

    /// Split `"<X> term, <Y> billing"` into its two halves. Free text, so this is a
    /// keyword scan rather than a grammar.
    pub fn parse(raw: &str) -> Self {
        let lower = raw.to_ascii_lowercase();
        let mut parts = lower.split(',');
        let term_part = parts.next().unwrap_or("");
        let cycle_part = parts.next().unwrap_or("");

        let duration = if term_part.contains("triennial") || term_part.contains("36") {
            TermDuration::Triennial
        } else if term_part.contains("annual") || term_part.contains("yearly") {
            TermDuration::Annual
        } else if term_part.contains("month") {
            TermDuration::Monthly
        } else {
            TermDuration::Unknown
        };

        let cycle = if cycle_part.contains("triennial") {
            BillingCycle::Triennial
        } else if cycle_part.contains("annual") || cycle_part.contains("yearly") {
            BillingCycle::Annual
        } else if cycle_part.contains("month") {
            BillingCycle::Monthly
        } else if cycle_part.contains("one") || cycle_part.contains("once") {
            BillingCycle::OneTime
        } else {
            BillingCycle::Unknown
        };

        BillingTerm { duration, cycle }
    }
}

/// When the partner can actually act on a finding.
///
/// Every finding must carry one. A recommendation that cannot be executed — "drop 12 seats"
/// on an annual commitment outside the cancellation window — is worse than no finding at
/// all: it is the specific failure that makes MSPs stop trusting tools like this one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RemediationWindow {
    /// Actionable today.
    Now,
    /// Locked in until the commitment ends; schedule the change now, it applies later.
    AtRenewal,
    /// Nothing can be done. Informational only.
    Never,
}

impl RemediationWindow {
    pub fn as_str(self) -> &'static str {
        match self {
            RemediationWindow::Now => "NOW",
            RemediationWindow::AtRenewal => "AT_RENEWAL",
            RemediationWindow::Never => "NEVER",
        }
    }
}

// ---------------------------------------------------------------------------------------
// ReconRow
// ---------------------------------------------------------------------------------------

/// One reconciliation line, typed.
///
/// Borrowing would be faster, but the callback contract (`&ReconRow`) already keeps this to
/// one live row at a time, so the allocations are bounded by row width, not file size.
#[derive(Debug, Clone, PartialEq)]
pub struct ReconRow {
    /// Zero-based index of the data record within the file (header excluded). This is the
    /// evidence anchor: every finding points back here.
    pub record_index: u64,

    pub partner_id: String,
    pub customer_id: String,
    pub customer_name: String,
    pub customer_domain_name: String,
    pub customer_country: String,
    pub invoice_number: String,
    pub order_id: String,

    pub product_id: String,
    pub sku_id: String,
    pub availability_id: String,
    pub sku_name: String,
    pub product_name: String,

    pub subscription_id: String,
    pub subscription_description: String,
    pub subscription_start_date: Option<NaiveDate>,
    pub subscription_end_date: Option<NaiveDate>,

    pub charge_start_date: Option<NaiveDate>,
    pub charge_end_date: Option<NaiveDate>,
    pub charge_type: ChargeType,

    /// Raw `TermAndBillingCycle` text, kept for evidence.
    pub term_and_billing_cycle: String,
    /// Parsed form of the above.
    pub term: BillingTerm,

    /// List price before adjustments.
    pub unit_price: Option<Decimal>,
    /// **The price actually charged.** All cost maths uses this, never `unit_price`.
    pub effective_unit_price: Option<Decimal>,
    /// Seats on the subscription. Informational.
    pub quantity: Option<Decimal>,
    /// **The seats actually billed on this line.** All cost maths uses this, never
    /// `quantity`: on a proration line `quantity` is the post-change subscription size
    /// while `billable_quantity` is the delta being charged for.
    pub billable_quantity: Option<Decimal>,
    pub subtotal: Option<Decimal>,
    pub tax_total: Option<Decimal>,
    pub total: Option<Decimal>,

    pub unit_type: String,
    pub currency: String,
    pub pricing_currency: String,
    pub pc_to_bc_exchange_rate: Option<Decimal>,

    pub price_adjustment_description: String,
    pub promotion_id: String,
    pub product_category: String,
    pub reference_id: ReferenceId,

    /// Columns present in the file that this build does not know about. Populated only
    /// when [`crate::ParseOptions::capture_unknown_columns`] is set; the *names* are always
    /// reported once in [`crate::StreamStats::unknown_columns`] regardless.
    pub unknown: Vec<(String, String)>,
}

impl ReconRow {
    /// Length of the charge period in whole days, inclusive of both endpoints.
    ///
    /// Inclusive because Partner Center's `ChargeEndDate` is the last billed day, not the
    /// exclusive upper bound. Getting this wrong shifts every prorated figure by one day.
    pub fn charge_days(&self) -> Option<i64> {
        let (s, e) = (self.charge_start_date?, self.charge_end_date?);
        Some((e - s).num_days() + 1)
    }

    /// True when the charge period covers roughly one calendar month.
    pub fn is_full_month_period(&self) -> bool {
        matches!(self.charge_days(), Some(28..=31))
    }

    /// `billable_quantity` forced to agree with the charge type's direction.
    ///
    /// Necessary because the export is inconsistent: credit lines usually arrive with the
    /// quantity *already* negative (`removeQuantity` + `'-2.0000`), but not always.
    /// Blindly multiplying by [`ChargeType::sign`] would flip those back to positive and
    /// turn a credit into a charge. This normalises instead of multiplying, so it is
    /// idempotent and safe to apply to either dialect.
    pub fn signed_billable_quantity(&self) -> Option<Decimal> {
        let q = self.billable_quantity?;
        Some(if self.charge_type.sign() < 0 { -q.abs() } else { q })
    }

    /// Cost of this line at the effective rate: `EffectiveUnitPrice × BillableQuantity`.
    ///
    /// Note this is *not* expected to equal `subtotal` on a proration line — there the
    /// export applies a day-fraction that this figure deliberately does not. Comparing the
    /// two is the basis of the `PRORATION_ANOMALY` detector, so they must stay separate.
    pub fn extended_cost(&self) -> Option<Decimal> {
        Some(self.effective_unit_price? * self.signed_billable_quantity()?)
    }

    /// Convenience for grouping: the pair that identifies a paid-for entitlement.
    pub fn subscription_key(&self) -> (&str, &str) {
        (self.customer_id.as_str(), self.subscription_id.as_str())
    }
}

impl Default for ReconRow {
    fn default() -> Self {
        ReconRow {
            record_index: 0,
            partner_id: String::new(),
            customer_id: String::new(),
            customer_name: String::new(),
            customer_domain_name: String::new(),
            customer_country: String::new(),
            invoice_number: String::new(),
            order_id: String::new(),
            product_id: String::new(),
            sku_id: String::new(),
            availability_id: String::new(),
            sku_name: String::new(),
            product_name: String::new(),
            subscription_id: String::new(),
            subscription_description: String::new(),
            subscription_start_date: None,
            subscription_end_date: None,
            charge_start_date: None,
            charge_end_date: None,
            charge_type: ChargeType::Other(String::new()),
            term_and_billing_cycle: String::new(),
            term: BillingTerm::UNKNOWN,
            unit_price: None,
            effective_unit_price: None,
            quantity: None,
            billable_quantity: None,
            subtotal: None,
            tax_total: None,
            total: None,
            unit_type: String::new(),
            currency: String::new(),
            pricing_currency: String::new(),
            pc_to_bc_exchange_rate: None,
            price_adjustment_description: String::new(),
            promotion_id: String::new(),
            product_category: String::new(),
            reference_id: ReferenceId::Absent,
            unknown: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn canonical_column_count_is_fixed() {
        assert_eq!(CANONICAL_COLUMNS.len(), 48);
        assert_eq!(CANONICAL_COLUMNS[col::EFFECTIVE_UNIT_PRICE], "EffectiveUnitPrice");
        assert_eq!(CANONICAL_COLUMNS[col::BILLABLE_QUANTITY], "BillableQuantity");
        assert_eq!(CANONICAL_COLUMNS[col::CHARGE_TYPE], "ChargeType");
        assert_eq!(CANONICAL_COLUMNS[col::REFERENCE_ID], "ReferenceId");
    }

    #[test]
    fn charge_type_is_case_insensitive() {
        for s in ["cycleCharge", "cyclecharge", "CycleCharge", "CYCLECHARGE", "cycle charge"] {
            assert_eq!(ChargeType::parse(s), ChargeType::CycleCharge, "{s}");
        }
        assert_eq!(ChargeType::parse("removeQuantity"), ChargeType::RemoveQuantity);
        assert_eq!(ChargeType::parse("cancelImmediate"), ChargeType::CancelImmediate);
        assert_eq!(
            ChargeType::parse("tierTransitionFee"),
            ChargeType::Other("tierTransitionFee".into())
        );
    }

    #[test]
    fn credit_charge_types_have_a_negative_sign() {
        assert_eq!(ChargeType::RemoveQuantity.sign(), -1);
        assert_eq!(ChargeType::CancelImmediate.sign(), -1);
        assert_eq!(ChargeType::Credit.sign(), -1);
        assert_eq!(ChargeType::CycleCharge.sign(), 1);
        assert_eq!(ChargeType::AddQuantity.sign(), 1);
        // An unmodelled charge type must never be assumed to be a credit.
        assert_eq!(ChargeType::Other("tierTransitionFee".into()).sign(), 1);
    }

    #[test]
    fn signed_quantity_does_not_double_negate() {
        let mut row = ReconRow { charge_type: ChargeType::RemoveQuantity, ..Default::default() };
        row.billable_quantity = Some(dec!(-2));
        assert_eq!(row.signed_billable_quantity(), Some(dec!(-2)));
        // Same line in the other dialect must land on the same answer.
        row.billable_quantity = Some(dec!(2));
        assert_eq!(row.signed_billable_quantity(), Some(dec!(-2)));
    }

    #[test]
    fn reference_id_reads_both_shapes() {
        let (r, failed) = ReferenceId::parse("8a7b6c5d-0000-4000-8000-0000000000a1");
        assert!(!failed);
        assert_eq!(r, ReferenceId::Legacy("8a7b6c5d-0000-4000-8000-0000000000a1".into()));

        let (r, failed) = ReferenceId::parse(r#"{"osId":"OS-77341","id":"abc","v":2}"#);
        assert!(!failed);
        assert_eq!(
            r,
            ReferenceId::V2 { os_id: Some("OS-77341".into()), id: Some("abc".into()), v: 2 }
        );
        assert_eq!(r.id(), Some("abc"));

        assert_eq!(ReferenceId::parse("").0, ReferenceId::Absent);

        // Malformed JSON degrades to a legacy scalar and flags the failure.
        let (r, failed) = ReferenceId::parse(r#"{"osId":"#);
        assert!(failed);
        assert!(matches!(r, ReferenceId::Legacy(_)));
    }

    #[test]
    fn term_and_billing_cycle_splits_into_two_axes() {
        let t = BillingTerm::parse("Annual term, Monthly billing");
        assert_eq!(t.duration, TermDuration::Annual);
        assert_eq!(t.cycle, BillingCycle::Monthly);

        let t = BillingTerm::parse("Monthly term, Monthly billing");
        assert_eq!(t.duration, TermDuration::Monthly);

        let t = BillingTerm::parse("Triennial term, Annual billing");
        assert_eq!(t.duration, TermDuration::Triennial);
        assert_eq!(t.cycle, BillingCycle::Annual);

        assert_eq!(BillingTerm::parse("").duration, TermDuration::Unknown);
    }

    #[test]
    fn charge_days_are_inclusive() {
        let row = ReconRow {
            charge_start_date: NaiveDate::from_ymd_opt(2026, 7, 1),
            charge_end_date: NaiveDate::from_ymd_opt(2026, 7, 31),
            ..Default::default()
        };
        assert_eq!(row.charge_days(), Some(31));
        assert!(row.is_full_month_period());
    }
}
