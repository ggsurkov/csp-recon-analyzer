//! Error types.
//!
//! Two tiers on purpose:
//!
//! * [`ReconError`] — the stream cannot continue (bad gzip, unreadable CSV, a column
//!   we genuinely cannot work without).
//! * [`RowError`] — one cell in one row did not parse. The row is still delivered with
//!   that field set to `None` and the problem is recorded in [`crate::StreamStats`].
//!
//! Microsoft changes this schema without notice. A recon file that is 99.99% intact must
//! produce 99.99% of the findings, not an error page.

use std::fmt;

/// A fatal error: the stream stopped.
#[derive(Debug, thiserror::Error)]
pub enum ReconError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("csv error: {0}")]
    Csv(#[from] csv::Error),

    /// The file had no header row at all.
    #[error("the file has no header row")]
    EmptyHeader,

    /// Columns the detectors cannot work without were absent from the header.
    #[error("reconciliation export is missing required column(s): {}", .0.join(", "))]
    MissingRequiredColumns(Vec<String>),

    /// Only raised when [`crate::ParseOptions::strict`] is on.
    #[error("row {}: {}", .0.record_index, .0)]
    StrictRow(Box<RowError>),
}

/// A cell contained text we refuse to guess at.
///
/// Deliberately not an `Option`: "this cell is empty" and "this cell says `n/a`" are
/// different facts, and only the second one is worth telling the user about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum CellError {
    #[error("not a decimal number")]
    NotDecimal,
    #[error("not a date in any accepted layout")]
    NotDate,
}

/// What went wrong inside a single cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowErrorKind {
    /// The cell did not parse as a decimal (after apostrophe/scientific handling).
    Decimal,
    /// The cell did not parse as a date in any accepted layout.
    Date,
    /// `ReferenceId` looked like JSON but was not valid JSON. Kept as a legacy scalar.
    ReferenceIdJson,
}

impl RowErrorKind {
    pub fn as_str(self) -> &'static str {
        match self {
            RowErrorKind::Decimal => "decimal",
            RowErrorKind::Date => "date",
            RowErrorKind::ReferenceIdJson => "reference_id_json",
        }
    }
}

/// A non-fatal, per-cell parse failure. Carries enough to reproduce the problem by hand.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowError {
    /// Zero-based index of the data record (header excluded).
    pub record_index: u64,
    /// Canonical column name, e.g. `EffectiveUnitPrice`.
    pub column: &'static str,
    /// The offending cell, verbatim and truncated to a sane length.
    pub value: String,
    pub kind: RowErrorKind,
}

impl RowError {
    pub(crate) fn new(
        record_index: u64,
        column: &'static str,
        value: &str,
        kind: RowErrorKind,
    ) -> Self {
        const MAX: usize = 120;
        let value = if value.len() > MAX {
            let mut end = MAX;
            while !value.is_char_boundary(end) {
                end -= 1;
            }
            format!("{}…", &value[..end])
        } else {
            value.to_owned()
        };
        Self { record_index, column, value, kind }
    }
}

impl fmt::Display for RowError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "column {} is not a valid {} (value: {:?})",
            self.column,
            self.kind.as_str(),
            self.value
        )
    }
}

impl std::error::Error for RowError {}
