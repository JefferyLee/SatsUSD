//! Rail-0: the RFQ atomic swap (spec 02 §7, journey J3).
//!
//! The degenerate base class — `oracle_spec = none`, price truth is
//! the two settlement signatures. LOCK and SETTLE collapse into one
//! co-signed transaction; refund machinery is unreachable.
//!
//! This crate's pure layer (manifest construction, swap planning,
//! fee math, the lock-template commitment) is I/O-free and fully
//! unit-tested. The tapd-driving builder and the devnet E2E live in
//! the follow-up increment (task: Rail-0 tapd builder + J3 E2E).

pub mod plan;
pub mod rail;
