//! The SatUSD oracle (spec 03 §3): dlcspecs-native BTC/USD tick
//! attestations from a single signer.
//!
//! Explicitly transitional (PRD §8 scaffolding: "Founder-run single
//! oracle"); the wire format is dlcspecs so any DLC client can
//! consume it, and the k-of-n upgrade changes the manifest, not the
//! format.

pub mod event;
pub mod oracle;
pub mod schnorr;
pub mod tlv;
