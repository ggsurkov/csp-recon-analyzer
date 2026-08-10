//! Deterministic leak detectors.
//!
//! House rules, enforced by review rather than by the compiler:
//!
//! * **No inference.** Every finding is arithmetic over cells that are in the file. If a
//!   detector cannot point at the rows it used, it does not ship.
//! * **Every finding carries a [`crate::RemediationWindow`].** Telling an MSP to drop seats
//!   they are contractually unable to drop is a P0 bug, not a cosmetic one.
//! * **Policy constants live in dated tables**, never as literals in a branch. Microsoft
//!   changes the rules; last quarter's file has to keep reprocessing correctly.
//! * **Suppress, never delete.** A finding below the noise threshold stays in the report
//!   and out of the headline. Silent removal is indistinguishable from a bug.
//!
//! Implemented: [`est`] (`EST_UPLIFT`).
//!
//! Planned, in the order they earn their keep: `DUPLICATE_SUBSCRIPTION`, `PROMO_EXPIRED`,
//! `DORMANT_AUTORENEW`, `PRORATION_ANOMALY`.

pub mod est;
