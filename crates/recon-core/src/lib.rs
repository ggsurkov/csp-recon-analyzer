//! # recon-core
//!
//! Streaming parser and deterministic leak detectors for **Microsoft Partner Center
//! license-based reconciliation exports** (direct CSP, NCE, 2026 schema).
//!
//! ## What it does
//!
//! Reads a `.csv.gz` recon export without loading it into memory, hands you one typed row
//! at a time, and runs detectors over the stream that turn the file into a list of
//! dollar-denominated findings — each one pointing back at the rows it came from.
//!
//! ```no_run
//! use recon_core::{stream_recon_gz, ParseOptions, detectors::est::EstDetector};
//!
//! let file = std::fs::File::open("recon.csv.gz")?;
//! let mut est = EstDetector::new();
//! let stats = stream_recon_gz(file, &ParseOptions::default(), |row| est.observe(row))?;
//!
//! let report = est.finish();
//! println!("{} rows, {} EST findings", stats.rows_parsed, report.findings.len());
//! for sub in report.reportable() {
//!     println!(
//!         "{:>10} {}/mo  {}  {}  [{}]",
//!         sub.monthly_run_rate, sub.currency, sub.customer_name, sub.sku_name,
//!         sub.remediation_window.as_str(),
//!     );
//! }
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! ## What it does not do
//!
//! No network calls. No Graph, no Partner Center API, no telemetry. This crate reads bytes
//! you already have — which is what lets the browser build promise that the file never
//! leaves the machine.
//!
//! ## Rules the code is held to
//!
//! * **Money is [`rust_decimal::Decimal`], never `f64`.** Claims are made to the cent about
//!   somebody's real invoice.
//! * **Cost is `EffectiveUnitPrice × BillableQuantity`.** `UnitPrice` is list, not what was
//!   charged; `Quantity` is the subscription size, not what this line billed for. Using
//!   either produces numbers that look plausible and are wrong on every prorated line.
//! * **Multi-member gzip.** Partner Center's export is a set of blobs. A single-member
//!   decoder reads the first one and reports a suspiciously small invoice.
//! * **Schema drift is survivable.** Unknown columns are reported, not fatal; a bad cell
//!   costs you that field, not the row.
//!
//! See the repository README for the reasoning behind each of these.

pub mod detectors;
pub mod error;
pub mod numeric;
pub mod parser;
pub mod types;

pub use error::{ReconError, RowError, RowErrorKind};
pub use numeric::{first_percentage, parse_date, parse_decimal, unguard};
pub use parser::{stream_recon_auto, stream_recon_csv, stream_recon_gz, ParseOptions, StreamStats};
pub use types::{
    BillingCycle, BillingTerm, ChargeType, ReconRow, ReferenceId, RemediationWindow, TermDuration,
    CANONICAL_COLUMNS,
};

pub use rust_decimal::Decimal;
