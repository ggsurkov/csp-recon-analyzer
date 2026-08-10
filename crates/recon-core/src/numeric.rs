//! Cell-level numeric and date parsing for Partner Center exports.
//!
//! Everything monetary goes through [`Decimal`]. There is no `f64` anywhere in this crate
//! and there must never be: a recon file is ~40k lines of 18-decimal prices, and we make
//! claims about a partner's money down to the cent. Binary floating point loses that
//! argument in front of a customer.
//!
//! The three shapes Partner Center actually emits:
//!
//! | on the wire                | meaning            |
//! |----------------------------|--------------------|
//! | `'22.000000000000000000`   | Excel-guard apostrophe, 18 decimals |
//! | `'0E-20`                   | scientific notation, usually in `TaxTotal` |
//! | `'-2.0000`                 | negative quantity on a `removeQuantity` line |
//!
//! The leading apostrophe exists so Excel does not reformat the cell. It is not part of
//! the value and must be stripped before parsing, not after.

use crate::error::CellError;
use chrono::NaiveDate;
use rust_decimal::Decimal;
use std::str::FromStr;

/// Strip the Excel guard and surrounding whitespace. Returns the bare numeric text.
///
/// Tolerant of repeated apostrophes and of whitespace on either side of them, both of
/// which show up in files that have been round-tripped through Excel.
#[inline]
pub fn unguard(raw: &str) -> &str {
    let mut s = raw.trim();
    while let Some(rest) = s.strip_prefix('\'') {
        s = rest.trim_start();
    }
    s.trim_end()
}

/// Parse a money/quantity cell.
///
/// Returns `Ok(None)` for an empty cell (a blank price is *absent*, not zero — the
/// difference matters when we later decide whether a line is billable).
/// Returns [`CellError`] when there is text we refuse to guess at, so the caller can record a
/// [`crate::RowError`] and keep going.
pub fn parse_decimal(raw: &str) -> Result<Option<Decimal>, CellError> {
    let s = unguard(raw);
    if s.is_empty() {
        return Ok(None);
    }
    parse_decimal_str(s).map(Some).ok_or(CellError::NotDecimal)
}

fn parse_decimal_str(s: &str) -> Option<Decimal> {
    // Scientific notation: `0E-20`, `1.5e+3`. `from_str` does not accept these.
    if s.as_bytes().iter().any(|b| *b == b'e' || *b == b'E') {
        return Decimal::from_scientific(s).ok();
    }
    // `from_str_exact` refuses to round; that is what we want for money. If the file ever
    // carries more than 28 significant digits we fall back to the rounding parser rather
    // than dropping the line — a value rounded at the 28th digit is still worth far more
    // than a hole in the ledger.
    Decimal::from_str_exact(s).ok().or_else(|| Decimal::from_str(s).ok())
}

/// Parse a date cell.
///
/// Partner Center writes `2026-07-01T00:00:00Z` in the Graph-exported files and plain
/// `2026-07-01` in some portal downloads; `MM/DD/YYYY` turns up when the file has been
/// opened and re-saved in Excel with a US locale. All three are accepted. Time-of-day is
/// discarded: reconciliation charge periods are whole days.
pub fn parse_date(raw: &str) -> Result<Option<NaiveDate>, CellError> {
    let s = unguard(raw);
    if s.is_empty() {
        return Ok(None);
    }
    // `2026-07-01T00:00:00Z`, `2026-07-01 00:00:00`, `2026-07-01`
    let head = s.split(['T', ' ']).next().unwrap_or(s);
    if let Ok(d) = NaiveDate::parse_from_str(head, "%Y-%m-%d") {
        return Ok(Some(d));
    }
    for layout in ["%m/%d/%Y", "%m/%d/%y", "%d.%m.%Y"] {
        if let Ok(d) = NaiveDate::parse_from_str(head, layout) {
            return Ok(Some(d));
        }
    }
    Err(CellError::NotDate)
}

/// Extract a whole-percent figure from free text, e.g. `"... 23% Fee Applied"` -> `23`.
///
/// Used to read the rate straight out of `PriceAdjustmentDescription` instead of inferring
/// it from a price ratio. Returns the first percentage found.
pub fn first_percentage(text: &str) -> Option<Decimal> {
    let bytes = text.as_bytes();
    let pct = bytes.iter().position(|b| *b == b'%')?;
    let mut start = pct;
    let mut seen_digit = false;
    while start > 0 {
        let c = bytes[start - 1];
        if c.is_ascii_digit() {
            seen_digit = true;
            start -= 1;
        } else if c == b'.' && seen_digit {
            start -= 1;
        } else {
            break;
        }
    }
    if !seen_digit {
        return None;
    }
    Decimal::from_str(text[start..pct].trim_end_matches('.')).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn strips_the_excel_guard() {
        assert_eq!(unguard("'22.000000000000000000"), "22.000000000000000000");
        assert_eq!(unguard("  ' -2.0000 "), "-2.0000");
        assert_eq!(unguard("''5"), "5");
        assert_eq!(unguard(""), "");
    }

    #[test]
    fn parses_the_three_wire_shapes() {
        assert_eq!(parse_decimal("'22.000000000000000000"), Ok(Some(dec!(22))));
        assert_eq!(parse_decimal("'-2.0000"), Ok(Some(dec!(-2))));
        assert_eq!(parse_decimal("'0E-20"), Ok(Some(Decimal::ZERO)));
        assert_eq!(parse_decimal("'1.5E+3"), Ok(Some(dec!(1500))));
        assert_eq!(parse_decimal(""), Ok(None));
        assert_eq!(parse_decimal("   "), Ok(None));
        assert_eq!(parse_decimal("'"), Ok(None));
        assert_eq!(parse_decimal("n/a"), Err(CellError::NotDecimal));
    }

    #[test]
    fn keeps_all_eighteen_decimals() {
        let d = parse_decimal("'-28.387096774193548387").unwrap().unwrap();
        assert_eq!(d.to_string(), "-28.387096774193548387");
        // The whole point: three of these sum back to an exact figure, unlike f64.
        assert_eq!(d * dec!(3), dec!(-85.161290322580645161));
    }

    #[test]
    fn parses_the_date_layouts() {
        let d = NaiveDate::from_ymd_opt(2026, 7, 1).unwrap();
        assert_eq!(parse_date("2026-07-01T00:00:00Z"), Ok(Some(d)));
        assert_eq!(parse_date("2026-07-01 00:00:00"), Ok(Some(d)));
        assert_eq!(parse_date("2026-07-01"), Ok(Some(d)));
        assert_eq!(parse_date("07/01/2026"), Ok(Some(d)));
        assert_eq!(parse_date(""), Ok(None));
        assert_eq!(parse_date("last tuesday"), Err(CellError::NotDate));
    }

    #[test]
    fn reads_percentages_out_of_prose() {
        assert_eq!(first_percentage("Extended Service Terms 23% Fee Applied"), Some(dec!(23)));
        assert_eq!(first_percentage("Extended Service Terms 3% Fee Applied"), Some(dec!(3)));
        assert_eq!(first_percentage("uplift of 3.5% applied"), Some(dec!(3.5)));
        assert_eq!(first_percentage("no numbers here"), None);
        assert_eq!(first_percentage("% leading"), None);
    }
}
