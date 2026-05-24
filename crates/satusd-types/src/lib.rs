//! SatUSD canonical types, encoding, domain separators, and hash derivations.
//!
//! Reference implementation for the M0 spec freeze (PRD §6, §18). The TypeScript
//! reference must produce byte-identical canonical encodings and hashes.

pub mod derive;
pub mod domain;
pub mod encoding;
pub mod hash;
pub mod tier;
pub mod types;

pub use encoding::{canonical_encode, Encode, Encoder};
pub use hash::{sha256, tagged_hash};
