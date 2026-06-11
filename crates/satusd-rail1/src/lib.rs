//! Rail-1: the single-oracle DLC rail (spec 02 §7, journey J4).
//!
//! This crate's pure layer: Schnorr adaptor signatures encrypted to
//! oracle anticipation points, and the CET bucket schedule built on
//! aligned binary digit-prefix wildcards (the power-of-two special
//! case of dlcspecs NumericOutcomeCompression).
//!
//! The funding-output construction (TA commitment leaf in a
//! Taproot+MuSig2 output, proposal 0001) and the devnet J4 E2E are
//! the next increment. v0 pre-signs under a single funding key held
//! by the test harness — MuSig2 aggregation is an explicit upgrade
//! point, not an omission.

pub mod adaptor;
pub mod cet;
