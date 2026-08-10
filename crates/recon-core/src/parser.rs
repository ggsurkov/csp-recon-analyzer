//! Streaming reader for reconciliation exports.
//!
//! Design constraints, in priority order:
//!
//! 1. **Never buffer the whole file.** Recon exports run to hundreds of thousands of lines
//!    and this crate compiles to WASM, where the whole file already sits in the tab's heap
//!    once. One row is live at a time; the caller gets it by reference.
//! 2. **Multi-member gzip.** Partner Center's export is a *set* of blobs. Concatenated,
//!    they form a multi-member gzip stream, and [`flate2::read::GzDecoder`] stops after the
//!    first member — silently, with no error. That is a data-loss bug that looks like a
//!    small invoice. [`MultiGzDecoder`] is the only correct choice here.
//! 3. **Survive schema drift.** The reader matches columns by normalised name, tolerates
//!    extra and reordered columns, and downgrades cell-level failures to warnings.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read};

use csv::{ReaderBuilder, StringRecord};
use flate2::bufread::MultiGzDecoder;

use crate::error::{ReconError, RowError, RowErrorKind};
use crate::numeric::{parse_date, parse_decimal};
use crate::types::{
    col, BillingTerm, ChargeType, ReconRow, ReferenceId, CANONICAL_COLUMNS, REQUIRED_COLUMNS,
};

/// Reader behaviour.
#[derive(Debug, Clone)]
pub struct ParseOptions {
    /// Keep the *values* of columns this build does not know about, per row, in
    /// [`ReconRow::unknown`]. Off by default: it doubles the allocation cost per row and
    /// most callers only need the column *names*, which are always reported.
    pub capture_unknown_columns: bool,
    /// Abort on the first cell-level parse failure instead of recording it and continuing.
    /// Useful in CI against a golden fixture; wrong for production files.
    pub strict: bool,
    /// Stop after this many data rows. For previewing a large upload.
    pub max_rows: Option<u64>,
    /// Cap on retained [`RowError`]s so a systematically broken column cannot exhaust
    /// memory. The *count* keeps rising after the cap.
    pub max_retained_row_errors: usize,
}

impl Default for ParseOptions {
    fn default() -> Self {
        ParseOptions {
            capture_unknown_columns: false,
            strict: false,
            max_rows: None,
            max_retained_row_errors: 100,
        }
    }
}

/// What the reader saw. Report this to the user — silently dropping rows is how a tool
/// like this loses trust permanently.
#[derive(Debug, Default, Clone)]
pub struct StreamStats {
    /// Data records handed to the callback.
    pub rows_parsed: u64,
    /// Records read from the CSV layer, including ones skipped or failed.
    pub records_read: u64,
    /// Records dropped entirely (unreadable at the CSV level).
    pub rows_failed: u64,
    /// Repeated header rows skipped — an artefact of concatenated per-blob exports.
    pub repeated_headers_skipped: u64,
    /// Header names present in the file that this build does not model. Reported once.
    pub unknown_columns: Vec<String>,
    /// Canonical columns absent from the header. Non-required ones only; a missing
    /// required column is a hard error.
    pub missing_columns: Vec<&'static str>,
    /// Cell-level failures, capped at [`ParseOptions::max_retained_row_errors`].
    pub row_errors: Vec<RowError>,
    /// Total cell-level failures, including ones not retained.
    pub row_error_count: u64,
}

impl StreamStats {
    /// True when the file parsed without a single cell-level complaint.
    pub fn is_clean(&self) -> bool {
        self.rows_failed == 0 && self.row_error_count == 0
    }
}

/// Header name -> canonical column index.
///
/// Normalisation strips the UTF-8 BOM, whitespace and every non-alphanumeric character,
/// then lowercases. `\u{feff}PartnerId`, `Partner Id` and `partner_id` all resolve to the
/// same column, which is what keeps the reader alive across Microsoft's cosmetic renames.
struct ColumnMap {
    /// `idx[canonical] = position in the record`.
    idx: [Option<usize>; CANONICAL_COLUMNS.len()],
    /// `(position, original header name)` for columns we do not model.
    unknown: Vec<(usize, String)>,
}

fn normalize_header(raw: &str) -> String {
    raw.trim_start_matches('\u{feff}')
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

impl ColumnMap {
    fn build(header: &StringRecord) -> Result<Self, ReconError> {
        if header.is_empty() {
            return Err(ReconError::EmptyHeader);
        }

        let canonical: HashMap<String, usize> = CANONICAL_COLUMNS
            .iter()
            .enumerate()
            .map(|(i, name)| (normalize_header(name), i))
            .collect();

        let mut idx = [None; CANONICAL_COLUMNS.len()];
        let mut unknown = Vec::new();
        for (pos, raw) in header.iter().enumerate() {
            let norm = normalize_header(raw);
            match canonical.get(&norm) {
                // First occurrence wins: a duplicated column name in the export must not
                // silently shadow the one we already bound.
                Some(&c) if idx[c].is_none() => idx[c] = Some(pos),
                Some(_) => {}
                None if norm.is_empty() => {}
                None => unknown.push((pos, raw.trim_start_matches('\u{feff}').to_owned())),
            }
        }

        let missing_required: Vec<String> = REQUIRED_COLUMNS
            .iter()
            .filter(|c| idx[**c].is_none())
            .map(|c| CANONICAL_COLUMNS[*c].to_owned())
            .collect();
        if !missing_required.is_empty() {
            return Err(ReconError::MissingRequiredColumns(missing_required));
        }

        Ok(ColumnMap { idx, unknown })
    }

    fn missing(&self) -> Vec<&'static str> {
        self.idx
            .iter()
            .enumerate()
            .filter(|(_, p)| p.is_none())
            .map(|(c, _)| CANONICAL_COLUMNS[c])
            .collect()
    }

    #[inline]
    fn get<'a>(&self, rec: &'a StringRecord, c: usize) -> &'a str {
        self.idx[c].and_then(|i| rec.get(i)).unwrap_or("").trim()
    }
}

/// Per-row scratch: collects cell errors without allocating when there are none.
struct RowSink<'a> {
    stats: &'a mut StreamStats,
    opts: &'a ParseOptions,
    record_index: u64,
    fatal: Option<RowError>,
}

impl RowSink<'_> {
    fn note(&mut self, column: usize, value: &str, kind: RowErrorKind) {
        let err = RowError::new(self.record_index, CANONICAL_COLUMNS[column], value, kind);
        if self.opts.strict && self.fatal.is_none() {
            self.fatal = Some(err.clone());
        }
        self.stats.row_error_count += 1;
        if self.stats.row_errors.len() < self.opts.max_retained_row_errors {
            self.stats.row_errors.push(err);
        }
    }

    fn dec(
        &mut self,
        map: &ColumnMap,
        rec: &StringRecord,
        c: usize,
    ) -> Option<rust_decimal::Decimal> {
        let raw = map.get(rec, c);
        match parse_decimal(raw) {
            Ok(v) => v,
            Err(_) => {
                self.note(c, raw, RowErrorKind::Decimal);
                None
            }
        }
    }

    fn date(&mut self, map: &ColumnMap, rec: &StringRecord, c: usize) -> Option<chrono::NaiveDate> {
        let raw = map.get(rec, c);
        match parse_date(raw) {
            Ok(v) => v,
            Err(_) => {
                self.note(c, raw, RowErrorKind::Date);
                None
            }
        }
    }
}

fn build_row(
    map: &ColumnMap,
    rec: &StringRecord,
    record_index: u64,
    sink: &mut RowSink<'_>,
) -> ReconRow {
    let s = |c: usize| map.get(rec, c).to_owned();

    let term_and_billing_cycle = s(col::TERM_AND_BILLING_CYCLE);
    let term = BillingTerm::parse(&term_and_billing_cycle);

    let reference_raw = map.get(rec, col::REFERENCE_ID);
    let (reference_id, json_failed) = ReferenceId::parse(reference_raw);
    if json_failed {
        sink.note(col::REFERENCE_ID, reference_raw, RowErrorKind::ReferenceIdJson);
    }

    let unknown = if sink.opts.capture_unknown_columns {
        map.unknown
            .iter()
            .map(|(pos, name)| (name.clone(), rec.get(*pos).unwrap_or("").to_owned()))
            .collect()
    } else {
        Vec::new()
    };

    ReconRow {
        record_index,
        partner_id: s(col::PARTNER_ID),
        customer_id: s(col::CUSTOMER_ID),
        customer_name: s(col::CUSTOMER_NAME),
        customer_domain_name: s(col::CUSTOMER_DOMAIN_NAME),
        customer_country: s(col::CUSTOMER_COUNTRY),
        invoice_number: s(col::INVOICE_NUMBER),
        order_id: s(col::ORDER_ID),

        product_id: s(col::PRODUCT_ID),
        sku_id: s(col::SKU_ID),
        availability_id: s(col::AVAILABILITY_ID),
        sku_name: s(col::SKU_NAME),
        product_name: s(col::PRODUCT_NAME),

        subscription_id: s(col::SUBSCRIPTION_ID),
        subscription_description: s(col::SUBSCRIPTION_DESCRIPTION),
        subscription_start_date: sink.date(map, rec, col::SUBSCRIPTION_START_DATE),
        subscription_end_date: sink.date(map, rec, col::SUBSCRIPTION_END_DATE),

        charge_start_date: sink.date(map, rec, col::CHARGE_START_DATE),
        charge_end_date: sink.date(map, rec, col::CHARGE_END_DATE),
        charge_type: ChargeType::parse(map.get(rec, col::CHARGE_TYPE)),

        term_and_billing_cycle,
        term,

        unit_price: sink.dec(map, rec, col::UNIT_PRICE),
        effective_unit_price: sink.dec(map, rec, col::EFFECTIVE_UNIT_PRICE),
        quantity: sink.dec(map, rec, col::QUANTITY),
        billable_quantity: sink.dec(map, rec, col::BILLABLE_QUANTITY),
        subtotal: sink.dec(map, rec, col::SUBTOTAL),
        tax_total: sink.dec(map, rec, col::TAX_TOTAL),
        total: sink.dec(map, rec, col::TOTAL),

        unit_type: s(col::UNIT_TYPE),
        currency: s(col::CURRENCY),
        pricing_currency: s(col::PRICING_CURRENCY),
        pc_to_bc_exchange_rate: sink.dec(map, rec, col::PC_TO_BC_EXCHANGE_RATE),

        price_adjustment_description: s(col::PRICE_ADJUSTMENT_DESCRIPTION),
        promotion_id: s(col::PROMOTION_ID),
        product_category: s(col::PRODUCT_CATEGORY),
        reference_id,

        unknown,
    }
}

/// Stream a **gzip-compressed** reconciliation export.
///
/// Handles multi-member gzip, which is what you get when Partner Center's blobs are
/// concatenated. `callback` is invoked once per data row, in file order.
///
/// ```no_run
/// # use recon_core::{stream_recon_gz, ParseOptions};
/// let file = std::fs::File::open("recon.csv.gz").unwrap();
/// let mut rows = 0u64;
/// let stats = stream_recon_gz(file, &ParseOptions::default(), |row| {
///     rows += row.billable_quantity.is_some() as u64;
/// }).unwrap();
/// assert_eq!(stats.rows_parsed, rows);
/// ```
pub fn stream_recon_gz<R, F>(
    reader: R,
    options: &ParseOptions,
    callback: F,
) -> Result<StreamStats, ReconError>
where
    R: Read,
    F: FnMut(&ReconRow),
{
    let decoder = MultiGzDecoder::new(BufReader::with_capacity(64 * 1024, reader));
    stream_recon_csv(decoder, options, callback)
}

/// Stream an **uncompressed** reconciliation export.
pub fn stream_recon_csv<R, F>(
    reader: R,
    options: &ParseOptions,
    mut callback: F,
) -> Result<StreamStats, ReconError>
where
    R: Read,
    F: FnMut(&ReconRow),
{
    let mut rdr = ReaderBuilder::new()
        .has_headers(true)
        // Microsoft adds columns without warning. A row that is one field longer than the
        // header must be read, not rejected.
        .flexible(true)
        .from_reader(BufReader::with_capacity(64 * 1024, reader));

    let map = ColumnMap::build(rdr.headers()?)?;

    let mut stats = StreamStats { missing_columns: map.missing(), ..Default::default() };
    stats.unknown_columns = map.unknown.iter().map(|(_, n)| n.clone()).collect();

    let mut rec = StringRecord::new();
    loop {
        match rdr.read_record(&mut rec) {
            Ok(false) => break,
            Ok(true) => {}
            Err(e) => {
                if options.strict {
                    return Err(e.into());
                }
                stats.rows_failed += 1;
                continue;
            }
        }
        stats.records_read += 1;

        // Concatenated per-blob exports repeat the header mid-stream. Skip it rather than
        // emitting a row whose every numeric field is garbage.
        if is_repeated_header(&rec) {
            stats.repeated_headers_skipped += 1;
            continue;
        }

        let record_index = stats.rows_parsed;
        let mut sink = RowSink { stats: &mut stats, opts: options, record_index, fatal: None };
        let row = build_row(&map, &rec, record_index, &mut sink);
        if let Some(err) = sink.fatal {
            return Err(ReconError::StrictRow(Box::new(err)));
        }

        stats.rows_parsed += 1;
        callback(&row);

        if options.max_rows.is_some_and(|m| stats.rows_parsed >= m) {
            break;
        }
    }

    Ok(stats)
}

/// Stream a file that may or may not be gzipped, deciding from the magic bytes.
///
/// The drag-and-drop entry point: users hand over `.csv`, `.csv.gz`, and `.csv` files that
/// are actually gzip with the extension stripped by a mail gateway.
pub fn stream_recon_auto<R, F>(
    reader: R,
    options: &ParseOptions,
    callback: F,
) -> Result<StreamStats, ReconError>
where
    R: Read,
    F: FnMut(&ReconRow),
{
    let mut buf = BufReader::with_capacity(64 * 1024, reader);
    let is_gzip = matches!(buf.fill_buf()?, [0x1f, 0x8b, ..]);
    if is_gzip {
        stream_recon_csv(MultiGzDecoder::new(buf), options, callback)
    } else {
        stream_recon_csv(buf, options, callback)
    }
}

fn is_repeated_header(rec: &StringRecord) -> bool {
    rec.get(0).is_some_and(|f| normalize_header(f) == "partnerid")
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    const HEADER: &str =
        "\u{feff}CustomerId,SubscriptionId,ChargeType,EffectiveUnitPrice,BillableQuantity";

    fn parse_all(csv: &str) -> (Vec<ReconRow>, StreamStats) {
        let mut rows = Vec::new();
        let stats =
            stream_recon_csv(csv.as_bytes(), &ParseOptions::default(), |r| rows.push(r.clone()))
                .unwrap();
        (rows, stats)
    }

    #[test]
    fn strips_the_bom_from_the_first_header_cell() {
        let (rows, _) = parse_all(&format!("{HEADER}\r\nC1,S1,cycleCharge,'10.00,'3\r\n"));
        assert_eq!(rows[0].customer_id, "C1");
        assert_eq!(rows[0].extended_cost(), Some(dec!(30)));
    }

    #[test]
    fn a_missing_required_column_is_fatal() {
        let err = stream_recon_csv(
            "CustomerId,SubscriptionId,ChargeType\r\nC1,S1,cycleCharge\r\n".as_bytes(),
            &ParseOptions::default(),
            |_| {},
        )
        .unwrap_err();
        assert!(matches!(err, ReconError::MissingRequiredColumns(ref c) if c.len() == 2));
    }

    #[test]
    fn unknown_columns_are_reported_not_fatal() {
        let csv =
            format!("{HEADER},NewMicrosoftColumn2026\r\nC1,S1,cycleCharge,'1,'1,surprise\r\n");
        let (rows, stats) = parse_all(&csv);
        assert_eq!(stats.unknown_columns, vec!["NewMicrosoftColumn2026"]);
        assert_eq!(rows.len(), 1);
        // Values are dropped unless explicitly requested.
        assert!(rows[0].unknown.is_empty());

        let mut kept = Vec::new();
        let opts = ParseOptions { capture_unknown_columns: true, ..Default::default() };
        stream_recon_csv(csv.as_bytes(), &opts, |r| kept.push(r.clone())).unwrap();
        assert_eq!(kept[0].unknown, vec![("NewMicrosoftColumn2026".into(), "surprise".into())]);
    }

    #[test]
    fn reordered_and_renamed_columns_still_bind() {
        let csv = "billable quantity,Charge_Type,SUBSCRIPTIONID,effectiveunitprice,customerid\r\n\
                   '4,cycleCharge,S1,'2.50,C1\r\n";
        let (rows, _) = parse_all(csv);
        assert_eq!(rows[0].customer_id, "C1");
        assert_eq!(rows[0].extended_cost(), Some(dec!(10)));
    }

    #[test]
    fn a_bad_cell_degrades_the_field_not_the_row() {
        let (rows, stats) = parse_all(&format!("{HEADER}\r\nC1,S1,cycleCharge,n/a,'3\r\n"));
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].effective_unit_price, None);
        assert_eq!(rows[0].billable_quantity, Some(dec!(3)));
        assert_eq!(stats.row_error_count, 1);
        assert_eq!(stats.row_errors[0].column, "EffectiveUnitPrice");
        assert!(!stats.is_clean());
    }

    #[test]
    fn strict_mode_turns_a_bad_cell_into_an_error() {
        let opts = ParseOptions { strict: true, ..Default::default() };
        let err = stream_recon_csv(
            format!("{HEADER}\r\nC1,S1,cycleCharge,n/a,'3\r\n").as_bytes(),
            &opts,
            |_| {},
        )
        .unwrap_err();
        assert!(matches!(err, ReconError::StrictRow(_)));
    }

    #[test]
    fn a_repeated_header_mid_stream_is_skipped() {
        // What you get when two per-blob exports are concatenated, each with its own header.
        let hdr =
            "PartnerId,CustomerId,SubscriptionId,ChargeType,EffectiveUnitPrice,BillableQuantity";
        let csv = format!(
            "\u{feff}{hdr}\r\nP1,C1,S1,cycleCharge,'1,'1\r\n{hdr}\r\nP1,C2,S2,cycleCharge,'1,'1\r\n"
        );
        let (rows, stats) = parse_all(&csv);
        assert_eq!(stats.repeated_headers_skipped, 1);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[1].customer_id, "C2");
        // Record indices stay contiguous: the skipped header does not consume one.
        assert_eq!(rows[1].record_index, 1);
    }

    #[test]
    fn max_rows_stops_early() {
        let mut csv = String::from(HEADER) + "\r\n";
        for i in 0..10 {
            csv.push_str(&format!("C{i},S{i},cycleCharge,'1,'1\r\n"));
        }
        let opts = ParseOptions { max_rows: Some(3), ..Default::default() };
        let mut n = 0;
        let stats = stream_recon_csv(csv.as_bytes(), &opts, |_| n += 1).unwrap();
        assert_eq!((n, stats.rows_parsed), (3, 3));
    }

    #[test]
    fn auto_detects_gzip_by_magic_bytes() {
        use flate2::{write::GzEncoder, Compression};
        use std::io::Write;

        let csv = format!("{HEADER}\r\nC1,S1,cycleCharge,'7,'2\r\n");
        let mut enc = GzEncoder::new(Vec::new(), Compression::fast());
        enc.write_all(csv.as_bytes()).unwrap();
        let gz = enc.finish().unwrap();

        for bytes in [csv.as_bytes(), gz.as_slice()] {
            let mut rows = Vec::new();
            stream_recon_auto(bytes, &ParseOptions::default(), |r| rows.push(r.clone())).unwrap();
            assert_eq!(rows[0].extended_cost(), Some(dec!(14)));
        }
    }
}
