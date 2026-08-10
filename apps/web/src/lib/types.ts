/**
 * Mirror of the payload `recon-wasm` returns. Keep in step with
 * `crates/recon-wasm/src/report.rs` — that file is the authority.
 */

/**
 * A monetary amount.
 *
 * `value` is exact. `display` is rounded to two places and carries the currency.
 * **Never call `Number()` on `value`** — a JS number is a float, and the entire Rust core
 * exists to keep these figures off floating point. If you need to compare amounts, note
 * that the Rust side has already sorted anything worth sorting.
 */
export interface Money {
  value: string;
  display: string;
}

export interface Finding {
  /** Row index in the source file — the evidence anchor. */
  record_index: number;

  customer_id: string;
  customer_name: string;
  subscription_id: string;
  sku_name: string;
  product_id: string;
  currency: string;

  charge_start_date: string | null;
  charge_end_date: string | null;

  /** Whole percent, e.g. `"23"`. */
  rate_percent: string;
  /** Ready to render, e.g. `"+23%"`. */
  rate_label: string;
  reason: string;

  base_unit_price: Money;
  effective_unit_price: Money;
  uplift_per_seat: Money;
  billable_quantity: string;
  uplift_amount: Money;
  monthly_run_rate: Money;

  confidence: string;
  confidence_label: string;
  evidence_kind: 'declared_and_priced' | 'declared' | 'price_ratio';
  price_adjustment_description: string;

  remediation_window: 'NOW' | 'AT_RENEWAL' | 'NEVER';
  action: string;

  /** Below the noise gate: render it, grey it out, leave it out of the headline. */
  suppressed: boolean;
}

export interface AnalysisResult {
  total_est_leak_monthly: Money;
  suppressed_monthly: Money;
  suppressed_subscription_count: number;
  noise_threshold_monthly: Money;
  currency: string;

  findings: Finding[];
  /** Subscriptions behind the headline total. Suppressed ones are counted separately. */
  reportable_subscription_count: number;

  rows_parsed: number;
  rows_failed: number;
  row_error_count: number;
  unknown_columns: string[];
  warnings: string[];

  execution_time_ms: number;
}
