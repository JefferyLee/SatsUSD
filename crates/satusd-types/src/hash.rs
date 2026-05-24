//! SHA-256 wrapper and domain-tagged hashing (PRD §18.1, §18.2).
//!
//! Poseidon (BN254) — used for StateRoot and SMT leaves — is deferred to the
//! next M0 chunk and intentionally not implemented here.

use sha2::{Digest, Sha256};

/// Plain SHA-256 of the concatenated parts.
pub fn sha256(parts: &[&[u8]]) -> [u8; 32] {
    let mut h = Sha256::new();
    for p in parts {
        h.update(p);
    }
    h.finalize().into()
}

/// Domain-tagged SHA-256: `SHA256(name_bytes || body)` (raw ASCII prefix, §18.2).
pub fn tagged_hash(name: &str, body: &[u8]) -> [u8; 32] {
    sha256(&[name.as_bytes(), body])
}
