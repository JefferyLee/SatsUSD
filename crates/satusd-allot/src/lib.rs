//! Stage-1 epoch allotment (spec 04 §2–3, ADR-0002).
//!
//! The founder runs this; **anyone re-runs it on the same public
//! data and must reproduce the plan byte-for-byte** — deviation is
//! public challengeable evidence. The script is the scaffold; the
//! formula is the law.

pub mod plan;
