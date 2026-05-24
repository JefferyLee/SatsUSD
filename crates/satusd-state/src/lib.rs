//! SatUSD state-node transition verifiers.
//!
//! Per PRD §8.1, every transition is first specified as a pure-Rust **software
//! verifier**; the zk circuit constraints are later derived from it. This crate
//! is that software layer for the accounting state machine.

pub mod mint;
