//! Live DA bundle format (PRD §10.3). The operator uploads this bundle *before*
//! `submit_claim`; the challenger fetches it to independently recompute the claim
//! (lineage / SPV / software verifier). The bundle's root is committed in the
//! `ReserveClaim` as `live_da_root`.
//!
//! Wire format (§10.3):
//! ```text
//! Header: magic "SUSDDA01" | version u8 | claim_id [32] | state_epoch u64
//!         | live_da_root [32] | bundle_size u32
//! Sections (ascending id): [id u8][size u32][canonical content] ...
//! ```
//!
//! `live_da_root = SHA256( concat( SHA256(section_wire_i) for each section ) )`,
//! where `section_wire_i = id || size_be || content` (PRD §10.3). The §10.1
//! phrasing "SHA256(canonical-encode(bundle))" is the looser, non-structural form
//! and is superseded by this per-section root (it is what lets a challenger detect
//! a *missing* section — DA-03 / DA-06); see ADR-008. All integers big-endian, to
//! match the §18.1 canonical encoding.

use satusd_types::hash::sha256;

pub const MAGIC: &[u8; 8] = b"SUSDDA01";
pub const VERSION: u8 = 1;

/// Section identifiers (PRD §10.3).
pub mod section {
    pub const REDEEM_INTENTS: u8 = 0x01;
    pub const LOCK_RECORDS: u8 = 0x02;
    pub const TA_LINEAGE_PROOFS: u8 = 0x03;
    pub const BTC_HTLC_PAYOUTS: u8 = 0x04;
    pub const BTC_PAYOUT_CONFIRMATIONS: u8 = 0x05;
    pub const LOCK_FINALIZES: u8 = 0x06;
    pub const UNIVERSE_BURN_PROOFS: u8 = 0x07;
    pub const ORACLE_MESSAGES: u8 = 0x08;
    pub const ORACLE_SOURCE_TRANSCRIPTS: u8 = 0x09;
    pub const L1_ANCHOR: u8 = 0x0A;
    pub const STATE_ROOT_PREV: u8 = 0x0B;
    pub const STATE_ROOT_NEW: u8 = 0x0C;
    pub const VERIFIER_TRANSCRIPT: u8 = 0x0D;
    pub const INDEX_AND_CHECKSUMS: u8 = 0x0E;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Section {
    pub id: u8,
    pub content: Vec<u8>,
}

impl Section {
    /// Wire bytes `id || size_be || content` — the unit `live_da_root` hashes.
    fn wire(&self) -> Vec<u8> {
        let mut w = Vec::with_capacity(5 + self.content.len());
        w.push(self.id);
        w.extend_from_slice(&(self.content.len() as u32).to_be_bytes());
        w.extend_from_slice(&self.content);
        w
    }

    fn hash(&self) -> [u8; 32] {
        sha256(&[&self.wire()])
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LiveDABundle {
    pub claim_id: [u8; 32],
    pub state_epoch: u64,
    /// Ascending-`id` sections (canonical order). A `submit_claim`'s sections are
    /// distinct, so `id` is a unique key.
    pub sections: Vec<Section>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum DaError {
    BadMagic,
    UnsupportedVersion(u8),
    Truncated,
    /// `bundle_size` did not match the section bytes that followed.
    SizeMismatch,
}

impl LiveDABundle {
    pub fn new(claim_id: [u8; 32], state_epoch: u64, mut sections: Vec<Section>) -> Self {
        sections.sort_by_key(|s| s.id);
        Self {
            claim_id,
            state_epoch,
            sections,
        }
    }

    /// `live_da_root` per §10.3.
    pub fn live_da_root(&self) -> [u8; 32] {
        let mut concat = Vec::with_capacity(self.sections.len() * 32);
        for s in &self.sections {
            concat.extend_from_slice(&s.hash());
        }
        sha256(&[&concat])
    }

    /// True if section `id` is present.
    pub fn has_section(&self, id: u8) -> bool {
        self.sections.iter().any(|s| s.id == id)
    }

    /// The content of section `id`, if present.
    pub fn section(&self, id: u8) -> Option<&[u8]> {
        self.sections
            .iter()
            .find(|s| s.id == id)
            .map(|s| s.content.as_slice())
    }

    fn sections_wire(&self) -> Vec<u8> {
        let mut w = Vec::new();
        for s in &self.sections {
            w.extend_from_slice(&s.wire());
        }
        w
    }

    /// Full bundle wire bytes (header + sections), with the header carrying the
    /// recomputed `live_da_root`.
    pub fn encode(&self) -> Vec<u8> {
        let sections = self.sections_wire();
        let mut out = Vec::with_capacity(85 + sections.len());
        out.extend_from_slice(MAGIC);
        out.push(VERSION);
        out.extend_from_slice(&self.claim_id);
        out.extend_from_slice(&self.state_epoch.to_be_bytes());
        out.extend_from_slice(&self.live_da_root());
        out.extend_from_slice(&(sections.len() as u32).to_be_bytes());
        out.extend_from_slice(&sections);
        out
    }

    /// Parse a bundle. Returns the bundle plus the `live_da_root` declared in the
    /// header (the challenger compares that — and the claim's committed root —
    /// against [`live_da_root`](Self::live_da_root); a tampered header or bundle
    /// surfaces as a mismatch rather than a parse error).
    pub fn decode(bytes: &[u8]) -> Result<(Self, [u8; 32]), DaError> {
        // Header is 8 + 1 + 32 + 8 + 32 + 4 = 85 bytes.
        if bytes.len() < 85 {
            return Err(DaError::Truncated);
        }
        if &bytes[0..8] != MAGIC {
            return Err(DaError::BadMagic);
        }
        let version = bytes[8];
        if version != VERSION {
            return Err(DaError::UnsupportedVersion(version));
        }
        let claim_id: [u8; 32] = bytes[9..41].try_into().unwrap();
        let state_epoch = u64::from_be_bytes(bytes[41..49].try_into().unwrap());
        let declared_root: [u8; 32] = bytes[49..81].try_into().unwrap();
        let bundle_size = u32::from_be_bytes(bytes[81..85].try_into().unwrap()) as usize;

        let body = &bytes[85..];
        if body.len() != bundle_size {
            return Err(DaError::SizeMismatch);
        }

        let mut sections = Vec::new();
        let mut i = 0;
        while i < body.len() {
            if i + 5 > body.len() {
                return Err(DaError::Truncated);
            }
            let id = body[i];
            let size = u32::from_be_bytes(body[i + 1..i + 5].try_into().unwrap()) as usize;
            let start = i + 5;
            let end = start
                .checked_add(size)
                .filter(|&e| e <= body.len())
                .ok_or(DaError::Truncated)?;
            sections.push(Section {
                id,
                content: body[start..end].to_vec(),
            });
            i = end;
        }
        Ok((
            Self {
                claim_id,
                state_epoch,
                sections,
            },
            declared_root,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> LiveDABundle {
        LiveDABundle::new(
            [0xab; 32],
            7,
            vec![
                Section {
                    id: section::REDEEM_INTENTS,
                    content: vec![1, 2, 3],
                },
                Section {
                    id: section::TA_LINEAGE_PROOFS,
                    content: vec![9, 9],
                },
                Section {
                    id: section::ORACLE_MESSAGES,
                    content: vec![4; 40],
                },
            ],
        )
    }

    #[test]
    fn new_sorts_sections_by_id() {
        let b = LiveDABundle::new(
            [0; 32],
            0,
            vec![
                Section {
                    id: 0x08,
                    content: vec![],
                },
                Section {
                    id: 0x01,
                    content: vec![],
                },
            ],
        );
        assert_eq!(
            b.sections.iter().map(|s| s.id).collect::<Vec<_>>(),
            [0x01, 0x08]
        );
    }

    #[test]
    fn encode_decode_roundtrips_and_root_matches() {
        let b = sample();
        let root = b.live_da_root();
        let bytes = b.encode();
        let (decoded, declared) = LiveDABundle::decode(&bytes).unwrap();
        assert_eq!(decoded, b);
        assert_eq!(declared, root);
        assert_eq!(decoded.live_da_root(), root);
    }

    #[test]
    fn root_is_section_sensitive() {
        let b = sample();
        let mut tampered = b.clone();
        tampered.sections[0].content[0] ^= 0xff;
        assert_ne!(b.live_da_root(), tampered.live_da_root());

        // Dropping a section (DA-03/DA-06 shape) changes the root too.
        let mut missing = b.clone();
        missing
            .sections
            .retain(|s| s.id != section::TA_LINEAGE_PROOFS);
        assert_ne!(b.live_da_root(), missing.live_da_root());
        assert!(!missing.has_section(section::TA_LINEAGE_PROOFS));
    }

    #[test]
    fn decode_rejects_bad_magic_and_truncation() {
        assert_eq!(LiveDABundle::decode(&[0u8; 10]), Err(DaError::Truncated));
        let mut bytes = sample().encode();
        bytes[0] ^= 0xff;
        assert_eq!(LiveDABundle::decode(&bytes), Err(DaError::BadMagic));
    }

    #[test]
    fn decode_detects_size_mismatch() {
        let mut bytes = sample().encode();
        bytes.push(0x00); // extra trailing byte not covered by bundle_size
        assert_eq!(LiveDABundle::decode(&bytes), Err(DaError::SizeMismatch));
    }
}
