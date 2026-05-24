//! SatUSD lock — Bitcoin-layer anchor output (PRD §5.D3).
//!
//! The lock anchor is a P2TR with a **NUMS internal key** (no key-path spend)
//! and a 2-leaf tapscript tree:
//!   finalize: OP_SHA256 <payment_hash> OP_EQUALVERIFY <operator> OP_CHECKSIGVERIFY <csv> OP_CSV
//!   refund:   <user_asset_refund_key> OP_CHECKSIGVERIFY <csv> OP_CSV
//!
//! The anchor internal key is derived from the dedicated domain
//! `SATUSD_LOCK_ANCHOR_NUMS_V1` (no salt) via the §18.7 NUMS rule. The domain is
//! registered in `satusd_types::domain` (§18.2) and pinned by the `nums_key`
//! cross-language test vector.

use bitcoin::hashes::Hash;
use bitcoin::opcodes::all::{OP_CHECKSIGVERIFY, OP_CSV, OP_EQUALVERIFY, OP_SHA256};
use bitcoin::script::{Builder, ScriptBuf};
use bitcoin::secp256k1::{Secp256k1, XOnlyPublicKey};
use bitcoin::taproot::{LeafVersion, TapLeafHash, TapNodeHash, TaprootBuilder, TaprootSpendInfo};

/// Domain for the lock anchor NUMS internal key (registered in §18.2).
pub const LOCK_ANCHOR_NUMS_DOMAIN: &str = satusd_types::domain::LOCK_ANCHOR_NUMS;

/// Asset-layer lock script key (§5.D3): `TapTweak(user_asset_refund_key,
/// SHA256("SATUSD_LOCK_TWEAK_V1" || redeem_intent_hash || payment_hash))`.
/// This is the Taproot Asset output's script key; the secp256k1 TapTweak is
/// off-circuit (§5.D18) and reuses the ADR-0014 primitive.
pub fn derive_lock_script_key(
    user_asset_refund_key: &[u8; 32],
    redeem_intent_hash: &[u8; 32],
    payment_hash: &[u8; 32],
) -> [u8; 32] {
    let tweak = satusd_types::derive::lock_tweak(redeem_intent_hash, payment_hash);
    satusd_crypto::nums::tap_tweak(user_asset_refund_key, &tweak)
}

/// The fixed NUMS internal key for lock anchors (unknown discrete log).
pub fn lock_anchor_internal_key() -> XOnlyPublicKey {
    let bytes = satusd_crypto::nums::derive_nums_key(LOCK_ANCHOR_NUMS_DOMAIN, &[]);
    XOnlyPublicKey::from_slice(&bytes).expect("NUMS key is a valid x-only pubkey")
}

/// Finalize tapleaf: spendable with the HTLC preimage + operator signature after CSV.
pub fn finalize_leaf(payment_hash: &[u8; 32], operator: XOnlyPublicKey, csv: i64) -> ScriptBuf {
    Builder::new()
        .push_opcode(OP_SHA256)
        .push_slice(payment_hash)
        .push_opcode(OP_EQUALVERIFY)
        .push_x_only_key(&operator)
        .push_opcode(OP_CHECKSIGVERIFY)
        .push_int(csv)
        .push_opcode(OP_CSV)
        .into_script()
}

/// Refund tapleaf: spendable by the user's asset-refund key after the (longer) CSV.
pub fn refund_leaf(user_asset_refund_key: XOnlyPublicKey, csv: i64) -> ScriptBuf {
    Builder::new()
        .push_x_only_key(&user_asset_refund_key)
        .push_opcode(OP_CHECKSIGVERIFY)
        .push_int(csv)
        .push_opcode(OP_CSV)
        .into_script()
}

/// A fully-built lock anchor: the Taproot spend info plus both leaf scripts.
pub struct LockAnchor {
    pub spend_info: TaprootSpendInfo,
    pub finalize_script: ScriptBuf,
    pub refund_script: ScriptBuf,
}

impl LockAnchor {
    /// The P2TR scriptPubKey to fund (the anchor output).
    pub fn script_pubkey(&self) -> ScriptBuf {
        ScriptBuf::new_p2tr_tweaked(self.spend_info.output_key())
    }

    /// 32-byte x-only Taproot output key.
    pub fn output_key_bytes(&self) -> [u8; 32] {
        self.spend_info.output_key().serialize()
    }

    /// Serialized tapd `tapscript_sibling` preimage for the finalize/refund branch
    /// (tapd `commitment/taproot.go` format): `0x01 || leftLeafTapHash ||
    /// rightLeafTapHash`. tapd recomputes the TapBranch hash with BIP341 sorting,
    /// so child order is irrelevant.
    pub fn tapscript_sibling_preimage(&self) -> Vec<u8> {
        let f = TapLeafHash::from_script(&self.finalize_script, LeafVersion::TapScript);
        let r = TapLeafHash::from_script(&self.refund_script, LeafVersion::TapScript);
        let mut out = Vec::with_capacity(65);
        out.push(0x01); // BranchPreimage
        out.extend_from_slice(f.as_byte_array());
        out.extend_from_slice(r.as_byte_array());
        out
    }
}

/// Build the §5.D3 Bitcoin-layer lock anchor (NUMS internal + finalize/refund leaves).
pub fn build_lock_anchor(
    payment_hash: &[u8; 32],
    operator: XOnlyPublicKey,
    user_asset_refund_key: XOnlyPublicKey,
    finalize_csv: i64,
    refund_csv: i64,
) -> LockAnchor {
    let finalize_script = finalize_leaf(payment_hash, operator, finalize_csv);
    let refund_script = refund_leaf(user_asset_refund_key, refund_csv);
    let secp = Secp256k1::verification_only();
    let spend_info = TaprootBuilder::new()
        .add_leaf(1, finalize_script.clone())
        .expect("finalize leaf at depth 1")
        .add_leaf(1, refund_script.clone())
        .expect("refund leaf at depth 1")
        .finalize(&secp, lock_anchor_internal_key())
        .expect("taproot tree finalizes");
    LockAnchor {
        spend_info,
        finalize_script,
        refund_script,
    }
}

/// Build the lock anchor as **tapd actually commits it** at the asset layer.
///
/// When tapd anchors a Taproot Asset with our finalize/refund branch supplied as
/// the `tapscript_sibling`, the real anchor tap tree is one level deeper than the
/// bare Bitcoin-layer demo:
///   root = TapBranch(ta_commitment_root, TapBranch(finalize, refund))
/// where `ta_commitment_root` is tapd's `taproot_asset_root` (read back from
/// ListUtxos / ListTransfers once the anchor confirms). The finalize/refund
/// control blocks therefore carry one extra merkle step (ta_commitment_root).
pub fn build_asset_lock_anchor(
    payment_hash: &[u8; 32],
    operator: XOnlyPublicKey,
    user_asset_refund_key: XOnlyPublicKey,
    finalize_csv: i64,
    refund_csv: i64,
    ta_commitment_root: &[u8; 32],
) -> LockAnchor {
    let finalize_script = finalize_leaf(payment_hash, operator, finalize_csv);
    let refund_script = refund_leaf(user_asset_refund_key, refund_csv);
    let secp = Secp256k1::verification_only();
    let ta_node = TapNodeHash::from_byte_array(*ta_commitment_root);
    let spend_info = TaprootBuilder::new()
        .add_hidden_node(1, ta_node)
        .expect("ta commitment node at depth 1")
        .add_leaf(2, finalize_script.clone())
        .expect("finalize leaf at depth 2")
        .add_leaf(2, refund_script.clone())
        .expect("refund leaf at depth 2")
        .finalize(&secp, lock_anchor_internal_key())
        .expect("taproot tree finalizes");
    LockAnchor {
        spend_info,
        finalize_script,
        refund_script,
    }
}

/// Convenience: build a lock anchor from raw x-only key bytes (for callers that
/// don't depend on rust-bitcoin types).
pub fn build_lock_anchor_from_bytes(
    payment_hash: &[u8; 32],
    operator_xonly: &[u8; 32],
    user_asset_refund_xonly: &[u8; 32],
    finalize_csv: i64,
    refund_csv: i64,
) -> Result<LockAnchor, bitcoin::secp256k1::Error> {
    let operator = XOnlyPublicKey::from_slice(operator_xonly)?;
    let user = XOnlyPublicKey::from_slice(user_asset_refund_xonly)?;
    Ok(build_lock_anchor(
        payment_hash,
        operator,
        user,
        finalize_csv,
        refund_csv,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use bitcoin::taproot::LeafVersion;

    fn xonly(domain: &str) -> XOnlyPublicKey {
        XOnlyPublicKey::from_slice(&satusd_crypto::nums::derive_nums_key(domain, &[])).unwrap()
    }

    fn sample() -> LockAnchor {
        build_lock_anchor(
            &[0x11; 32],
            xonly("test-operator"),
            xonly("test-user"),
            150,
            288,
        )
    }

    #[test]
    fn lock_script_key_is_deterministic_and_sensitive() {
        let refund = satusd_crypto::nums::derive_nums_key("test-user", &[]);
        let rih = [0x22; 32];
        let ph = [0x33; 32];
        let k = derive_lock_script_key(&refund, &rih, &ph);
        assert_eq!(k, derive_lock_script_key(&refund, &rih, &ph));
        // It is a valid x-only key (TapTweak output).
        assert!(satusd_crypto::nums::is_valid_xonly(&k));
        // Changing the payment hash changes the key.
        let mut ph2 = ph;
        ph2[0] ^= 1;
        assert_ne!(k, derive_lock_script_key(&refund, &rih, &ph2));
    }

    #[test]
    fn lock_anchor_is_deterministic() {
        assert_eq!(sample().output_key_bytes(), sample().output_key_bytes());
    }

    #[test]
    fn both_leaves_have_control_blocks_and_differ() {
        let a = sample();
        assert_ne!(a.finalize_script, a.refund_script);
        assert!(a
            .spend_info
            .control_block(&(a.finalize_script.clone(), LeafVersion::TapScript))
            .is_some());
        assert!(a
            .spend_info
            .control_block(&(a.refund_script.clone(), LeafVersion::TapScript))
            .is_some());
    }

    #[test]
    fn asset_anchor_adds_one_merkle_step_over_bare() {
        // The asset-layer tree is TapBranch(ta_root, branch(finalize, refund)),
        // one level deeper than the bare anchor, so the finalize control block
        // carries exactly one extra 32-byte merkle step.
        let bare = sample();
        let asset = build_asset_lock_anchor(
            &[0x11; 32],
            xonly("test-operator"),
            xonly("test-user"),
            150,
            288,
            &[0xcd; 32],
        );
        let leaf = (asset.finalize_script.clone(), LeafVersion::TapScript);
        let bare_cb = bare
            .spend_info
            .control_block(&(bare.finalize_script.clone(), LeafVersion::TapScript))
            .unwrap();
        let asset_cb = asset.spend_info.control_block(&leaf).unwrap();
        assert_eq!(asset_cb.serialize().len(), bare_cb.serialize().len() + 32);
        assert_eq!(asset.spend_info.internal_key(), lock_anchor_internal_key());
    }

    #[test]
    fn internal_key_is_nums_and_spk_is_p2tr() {
        let a = sample();
        // Internal key is the lock-anchor NUMS (no key-path spend possible).
        assert_eq!(a.spend_info.internal_key(), lock_anchor_internal_key());
        assert!(a.script_pubkey().is_p2tr());
        println!(
            "lock anchor output_key = 0x{}",
            hex::encode(a.output_key_bytes())
        );
    }
}
