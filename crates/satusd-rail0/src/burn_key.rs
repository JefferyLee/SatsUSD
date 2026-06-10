//! tapd-native burn key derivation (spec 01 §4).
//!
//! Replicates `asset.DeriveBurnKey` from lightninglabs/taproot-assets:
//!
//! ```text
//! data    = outpoint(txid_wire ‖ vout_le) ‖ asset_id ‖ script_key_xonly
//! burnKey = ComputeTaprootOutputKey(NUMS, data)
//!         = NUMS + tagged_hash("TapTweak", NUMS_x ‖ data)·G   (x-only)
//! ```
//!
//! The per-burn uniqueness comes from the first input's PrevID; the
//! provable unspendability from the tapd NUMS point ("taproot-assets"
//! try-and-increment). Equivalence with tapd is asserted by the
//! devnet integration test (`tests/devnet_burn_key.rs`), which
//! compares this derivation against a live `BurnAsset` call.

use satusd_crypto::nums::tap_tweak_data;

/// x-coordinate of tapd's NUMS public key
/// (`asset.NUMSBytes = 027c79…53fe`, even parity).
pub const TAPD_NUMS_X: [u8; 32] = [
    0x7c, 0x79, 0xb9, 0xb2, 0x6e, 0x46, 0x38, 0x95, 0xee, 0xf5, 0x67, 0x9d, 0x85, 0x58, 0x94, 0x2c,
    0x86, 0xc4, 0xad, 0x22, 0x33, 0xad, 0xef, 0x01, 0xbc, 0x3e, 0x6d, 0x54, 0x0b, 0x36, 0x53, 0xfe,
];

/// The first input's PrevID, per tapd semantics.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrevId {
    /// Anchor txid in **wire byte order** (the reverse of the
    /// display hex shown by explorers and tapd's `anchor_point`).
    pub txid_wire: [u8; 32],
    pub vout: u32,
    pub asset_id: [u8; 32],
    /// The spent asset's script key, x-only (drop the parity byte of
    /// tapd's 33-byte serialization).
    pub script_key_xonly: [u8; 32],
}

/// The burn output's asset script key for a burn whose first input
/// is `prev`. x-only, parity discarded — exactly tapd's return.
pub fn derive_burn_key(prev: &PrevId) -> [u8; 32] {
    let mut data = Vec::with_capacity(36 + 32 + 32);
    data.extend_from_slice(&prev.txid_wire);
    data.extend_from_slice(&prev.vout.to_le_bytes());
    data.extend_from_slice(&prev.asset_id);
    data.extend_from_slice(&prev.script_key_xonly);
    tap_tweak_data(&TAPD_NUMS_X, &data)
}

/// Parse tapd's `anchor_point` display string ("<display-hex>:<vout>")
/// into wire-order txid + vout.
pub fn parse_anchor_point(s: &str) -> Option<([u8; 32], u32)> {
    let (txid_hex, vout) = s.split_once(':')?;
    if txid_hex.len() != 64 {
        return None;
    }
    let mut txid = [0u8; 32];
    for (i, byte) in txid.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&txid_hex[2 * i..2 * i + 2], 16).ok()?;
    }
    txid.reverse(); // display → wire order
    Some((txid, vout.parse().ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prev() -> PrevId {
        PrevId {
            txid_wire: [0xaa; 32],
            vout: 1,
            asset_id: [0xbb; 32],
            script_key_xonly: TAPD_NUMS_X, // any valid x-only works
        }
    }

    #[test]
    fn deterministic_and_field_sensitive() {
        let k = derive_burn_key(&prev());
        assert_eq!(k, derive_burn_key(&prev()));

        let mut p = prev();
        p.vout = 2;
        assert_ne!(k, derive_burn_key(&p), "vout must change the key");

        let mut p = prev();
        p.txid_wire[0] ^= 1;
        assert_ne!(k, derive_burn_key(&p), "txid must change the key");

        let mut p = prev();
        p.asset_id[0] ^= 1;
        assert_ne!(k, derive_burn_key(&p), "asset_id must change the key");

        let mut p = prev();
        p.script_key_xonly = derive_burn_key(&prev()); // some other valid key
        assert_ne!(k, derive_burn_key(&p), "script key must change the key");
    }

    #[test]
    fn anchor_point_parsing_reverses_to_wire_order() {
        let display = format!("{}:{}", "11".repeat(31) + "22", 3);
        let (txid, vout) = parse_anchor_point(&display).unwrap();
        assert_eq!(vout, 3);
        assert_eq!(txid[0], 0x22, "display hex is reversed into wire order");
        assert_eq!(txid[31], 0x11);
        assert!(parse_anchor_point("nonsense").is_none());
        assert!(parse_anchor_point("aabb:1").is_none());
    }

    /// Pinned self-vector: cross-language ground truth for the TS
    /// mirror, and a regression guard on the derivation. Equivalence
    /// with tapd itself is the devnet test's job.
    #[test]
    fn pinned_vector() {
        assert_eq!(
            hex::encode(derive_burn_key(&prev())),
            "f855f4942ec00540ea3b7df97e14a366fb1d842437aa5d18c6b1e7647d987bff"
        );
    }
}
