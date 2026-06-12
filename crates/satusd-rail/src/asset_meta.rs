//! Genesis `AssetMeta` (spec 01 §2.2) — the canonical bytes carried
//! in the TA genesis metadata field. Pinned cross-language before
//! any signet/mainnet mint (spec 01 §8 precondition).

use crate::encode::Encoder;

/// Spec 01 version at genesis.
pub const ASSET_SPEC_VERSION: u16 = 1;
pub const ASSET_NAME: &str = "SatUSD";
pub const ASSET_DECIMALS: u8 = 6;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssetMeta {
    pub spec_version: u16,
    pub name: String,
    pub decimals: u8,
    /// SHA-256 of `docs/MISSION.md` at genesis — a historical
    /// anchor, not a living reference.
    pub mission_commitment: [u8; 32],
}

impl AssetMeta {
    pub fn genesis(mission_commitment: [u8; 32]) -> Self {
        Self {
            spec_version: ASSET_SPEC_VERSION,
            name: ASSET_NAME.into(),
            decimals: ASSET_DECIMALS,
            mission_commitment,
        }
    }

    pub fn canonical_encode(&self) -> Vec<u8> {
        let mut e = Encoder::new();
        e.put_u16(self.spec_version);
        e.put_string(&self.name);
        e.put_u8(self.decimals);
        e.put_bytes32(&self.mission_commitment);
        e.into_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_is_exact() {
        let m = AssetMeta::genesis([0xab; 32]);
        let b = m.canonical_encode();
        // u16 ‖ varbytes("SatUSD") ‖ u8 ‖ 32 bytes
        assert_eq!(b.len(), 2 + 1 + 6 + 1 + 32);
        assert_eq!(&b[..2], &[0x00, 0x01]);
        assert_eq!(b[2], 6); // bigsize len of "SatUSD"
        assert_eq!(&b[3..9], b"SatUSD");
        assert_eq!(b[9], 6); // decimals
        assert_eq!(&b[10..], &[0xab; 32]);
    }

    /// Pinned vector: cross-language ground truth (spec 01 §8).
    #[test]
    fn pinned_vector() {
        let m = AssetMeta::genesis([0x11; 32]);
        assert_eq!(
            hex::encode(m.canonical_encode()),
            "000106536174555344061111111111111111111111111111111111111111111111111111111111111111"
        );
    }
}
