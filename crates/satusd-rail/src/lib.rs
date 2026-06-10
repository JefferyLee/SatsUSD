//! Rail standard core (spec 02) on the shared conventions (spec 00).
//!
//! Owns: the canonical encoding primitives, `RailManifest` and its
//! self-certifying `rail_id`, the `Quote` payload, the conversion
//! phase machine, and the capacity formula (ADR-0002).

pub mod capacity;
pub mod encode;
pub mod manifest;
pub mod quote;
pub mod state;
