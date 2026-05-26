//! Operator BTC HTLC (PRD §5.D3 atomic-swap counterpart, §18.6). The operator
//! locks BTC in a P2TR with a NUMS internal key (no key-path spend) and a 2-leaf
//! tapscript tree:
//!   claim:  OP_SHA256 <payment_hash> OP_EQUALVERIFY <user_claim_key> OP_CHECKSIG
//!   refund: <operator_key> OP_CHECKSIGVERIFY <btc_htlc_csv> OP_CSV
//!
//! The user claims by revealing the same `payment_hash` preimage that unlocks the
//! SatUSD lock's finalize leaf — making the two-leg redemption atomic. The
//! operator can reclaim only after `btc_htlc_csv` (which §5.D3 requires to be
//! strictly shorter than the asset-lock CSV, so the user always has time to
//! refund the SatUSD if the swap stalls).

use bitcoin::opcodes::all::{OP_CHECKSIG, OP_CHECKSIGVERIFY, OP_CSV, OP_EQUALVERIFY, OP_SHA256};
use bitcoin::script::{Builder, ScriptBuf};
use bitcoin::secp256k1::{Secp256k1, XOnlyPublicKey};
use bitcoin::taproot::{TaprootBuilder, TaprootSpendInfo};

/// The operator's BTC HTLC output.
pub struct BtcHtlc {
    pub spend_info: TaprootSpendInfo,
    pub claim_script: ScriptBuf,
    pub refund_script: ScriptBuf,
}

impl BtcHtlc {
    pub fn script_pubkey(&self) -> ScriptBuf {
        ScriptBuf::new_p2tr_tweaked(self.spend_info.output_key())
    }
    pub fn output_key_bytes(&self) -> [u8; 32] {
        self.spend_info.output_key().serialize()
    }
}

/// Claim leaf: spendable by revealing the `payment_hash` preimage + a `user_claim`
/// signature.
pub fn claim_leaf(payment_hash: &[u8; 32], user_claim: XOnlyPublicKey) -> ScriptBuf {
    Builder::new()
        .push_opcode(OP_SHA256)
        .push_slice(payment_hash)
        .push_opcode(OP_EQUALVERIFY)
        .push_x_only_key(&user_claim)
        .push_opcode(OP_CHECKSIG)
        .into_script()
}

/// Refund leaf: spendable by the operator after the `btc_htlc_csv` relative
/// timelock.
pub fn refund_leaf(operator: XOnlyPublicKey, btc_htlc_csv: i64) -> ScriptBuf {
    Builder::new()
        .push_x_only_key(&operator)
        .push_opcode(OP_CHECKSIGVERIFY)
        .push_int(btc_htlc_csv)
        .push_opcode(OP_CSV)
        .into_script()
}

/// Build the operator BTC HTLC anchor. The internal key is a NUMS point derived
/// from the `payment_hash` (no key-path spend; both legs go through the explicit
/// leaves) under the registered `SATUSD_BTC_HTLC_NUMS_V1` domain (§18.2).
pub fn build_btc_htlc(
    payment_hash: &[u8; 32],
    user_claim: XOnlyPublicKey,
    operator: XOnlyPublicKey,
    btc_htlc_csv: i64,
) -> BtcHtlc {
    let claim = claim_leaf(payment_hash, user_claim);
    let refund = refund_leaf(operator, btc_htlc_csv);
    let nums = XOnlyPublicKey::from_slice(&satusd_crypto::nums::derive_nums_key(
        satusd_types::domain::BTC_HTLC_NUMS,
        payment_hash,
    ))
    .expect("NUMS x-only");
    let secp = Secp256k1::verification_only();
    let spend_info = TaprootBuilder::new()
        .add_leaf(1, claim.clone())
        .unwrap()
        .add_leaf(1, refund.clone())
        .unwrap()
        .finalize(&secp, nums)
        .expect("valid 2-leaf taproot");
    BtcHtlc {
        spend_info,
        claim_script: claim,
        refund_script: refund,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bitcoin::hashes::{sha256, Hash};
    use bitcoin::secp256k1::{Keypair, SecretKey};

    fn xonly(label: &str) -> XOnlyPublicKey {
        let secp = Secp256k1::new();
        let sk =
            SecretKey::from_slice(&sha256::Hash::hash(label.as_bytes()).to_byte_array()).unwrap();
        Keypair::from_secret_key(&secp, &sk).x_only_public_key().0
    }

    #[test]
    fn htlc_is_deterministic_and_distinct_per_hash() {
        let u = xonly("user");
        let o = xonly("op");
        let h1 = build_btc_htlc(&[0x11; 32], u, o, 144);
        let h2 = build_btc_htlc(&[0x11; 32], u, o, 144);
        let h3 = build_btc_htlc(&[0x22; 32], u, o, 144);
        assert_eq!(h1.output_key_bytes(), h2.output_key_bytes()); // deterministic
        assert_ne!(h1.output_key_bytes(), h3.output_key_bytes()); // unique per hash
                                                                  // The output is a valid P2TR.
        assert!(h1.script_pubkey().is_p2tr());
    }
}
