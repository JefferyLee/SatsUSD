//! The client verification library (FR-7, PRD §4): everything a
//! wallet needs to check protocol claims **with no access to any
//! server of ours** — pure functions over artifacts plus a Bitcoin
//! view the caller supplies.
//!
//! v0 scope: Rail-0 settlement verification (S3 artifacts →
//! settlement points) and the internal_twap marker computation
//! (spec 03 §5.2, normative ahead of activation). Lineage-proof
//! integration (`satusd-ta-proof`) and supply-commitment
//! reconstruction (spec 01 §5) are the next increments.

pub mod marker;
pub mod position;
pub mod settlement;
