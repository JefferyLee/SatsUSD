//! The Rail-1 funding output (proposal 0001 §3.1): one P2TR output
//! that is simultaneously a TA-committed output and a DLC funding
//! output.
//!
//! ```text
//! internal key  P  = funding key (v0 single key; MuSig2 upgrade)
//! script tree   T  = { ta_commit_leaf, refund_leaf }
//! output key    Q  = P + TapTweak(P ‖ MerkleRoot(T))·G
//! spending      key-path: CET via adaptor (02 §3.3)
//!               script-path: refund after CSV (02 §3.4)
//! ```
//!
//! The TA commitment leaf is opaque here — tapd computes it when the
//! asset lands (NewAddr's `tapscript_sibling` carries our refund
//! leaf; tapd builds the branch). This module owns the tree math,
//! the output-key reconstruction a verifier needs, and the
//! keyspend-secret tweak that feeds `adaptor::presign`.
//!
//! Note the dual-layer reality (proposal 0001): the BTC-level
//! keyspend moves the sats; the TA-level transfer of the committed
//! asset is authorized separately by the asset script key. This
//! module is the BTC layer; the asset-level CET vPSBTs are the
//! devnet increment's work.

use bitcoin::hashes::Hash;
use bitcoin::key::Secp256k1 as BtcSecp;
use bitcoin::taproot::{LeafVersion, TapLeafHash, TapNodeHash, TaprootBuilder};
use bitcoin::ScriptBuf;
use satusd_crypto::nums::tap_tweak_data;
use satusd_rail::encode::tagged_hash;
use secp256k1::{Parity, Scalar, Secp256k1, SecretKey};

/// The refund leaf (tapscript — CHECKMULTISIG is disabled in
/// tapscript, so CHECKSIGVERIFY/CHECKSIG):
///
/// `<delta> OP_CSV OP_DROP <user_x> OP_CHECKSIGVERIFY <lp_x> OP_CHECKSIG`
pub fn refund_leaf_script(csv_delta: u16, user_x: &[u8; 32], lp_x: &[u8; 32]) -> ScriptBuf {
    bitcoin::script::Builder::new()
        .push_int(i64::from(csv_delta))
        .push_opcode(bitcoin::opcodes::all::OP_CSV)
        .push_opcode(bitcoin::opcodes::all::OP_DROP)
        .push_slice(user_x)
        .push_opcode(bitcoin::opcodes::all::OP_CHECKSIGVERIFY)
        .push_slice(lp_x)
        .push_opcode(bitcoin::opcodes::all::OP_CHECKSIG)
        .into_script()
}

/// The **holder-only** refund leaf — the offline maturity-floor CSV
/// fallback (spec 07 §6): after `csv_delta` blocks the **holder alone**
/// recovers `Q`, with no LP. This is the unconditional safety net behind
/// the unilateral maturity settlement; the LP is incentivised to broadcast
/// the maturity CET (claiming its change) before this opens.
///
/// `<delta> OP_CSV OP_DROP <holder_x> OP_CHECKSIG`
pub fn refund_leaf_holder_only(csv_delta: u16, holder_x: &[u8; 32]) -> ScriptBuf {
    bitcoin::script::Builder::new()
        .push_int(i64::from(csv_delta))
        .push_opcode(bitcoin::opcodes::all::OP_CSV)
        .push_opcode(bitcoin::opcodes::all::OP_DROP)
        .push_slice(holder_x)
        .push_opcode(bitcoin::opcodes::all::OP_CHECKSIG)
        .into_script()
}

/// The funding output's tree facts, reconstructable by any verifier
/// from (internal key, TA leaf hash, refund script).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FundingOutput {
    /// x-only internal key (the funding key).
    pub internal_x: [u8; 32],
    /// BIP-341 merkle root over {ta_leaf, refund_leaf}.
    pub merkle_root: [u8; 32],
    /// x-only output key Q — what the on-chain scriptPubKey carries.
    pub output_x: [u8; 32],
}

/// Build the funding output facts. `ta_leaf_hash` is the TA
/// commitment leaf hash (from tapd's proof data / ManagedUtxo);
/// `refund_script` is ours.
pub fn funding_output(
    internal_x: &[u8; 32],
    ta_leaf_hash: &[u8; 32],
    refund_script: &ScriptBuf,
) -> FundingOutput {
    let refund_leaf = TapLeafHash::from_script(refund_script, LeafVersion::TapScript);
    let ta_node = TapNodeHash::from_byte_array(*ta_leaf_hash);
    let root = TapNodeHash::from_node_hashes(ta_node, refund_leaf.into());
    let root_bytes = root.to_byte_array();
    FundingOutput {
        internal_x: *internal_x,
        merkle_root: root_bytes,
        output_x: tap_tweak_data(internal_x, &root_bytes),
    }
}

/// The keyspend secret for the funding output: `d' = even_y(d) + t`
/// with `t = H_TapTweak(P.x ‖ root)`. Feed the result to
/// `adaptor::presign` — CETs are keyspends of Q.
pub fn keyspend_secret(
    internal_sk: &[u8; 32],
    merkle_root: &[u8; 32],
) -> Result<[u8; 32], secp256k1::Error> {
    let secp = Secp256k1::new();
    let mut d = SecretKey::from_byte_array(*internal_sk)?;
    let (px, parity) = d.x_only_public_key(&secp);
    if parity == Parity::Odd {
        d = d.negate();
    }
    let mut msg = Vec::with_capacity(64);
    msg.extend_from_slice(&px.serialize());
    msg.extend_from_slice(merkle_root);
    let t = Scalar::from_be_bytes(tagged_hash("TapTweak", &msg))
        .map_err(|_| secp256k1::Error::InvalidTweak)?;
    Ok(d.add_tweak(&t)?.secret_bytes())
}

/// The serialized tapscript sibling preimage NewAddr expects
/// (tapd `commitment.TapscriptPreimage`, leaf type):
///
/// `[0x00 LeafPreimage] ‖ [0xc0 leaf version] ‖ compact_size(script) ‖ script`
///
/// Validated live by the devnet funding test; tapd additionally
/// rejects leaves that look like TA commitments (ours doesn't).
pub fn sibling_preimage(refund_script: &ScriptBuf) -> Vec<u8> {
    let script = refund_script.as_bytes();
    let mut out = Vec::with_capacity(2 + 9 + script.len());
    out.push(0x00); // LeafPreimage
    out.push(0xc0); // tapscript leaf version
                    // Bitcoin compact size for the script length.
    let len = script.len() as u64;
    match len {
        0..=0xfc => out.push(len as u8),
        0xfd..=0xffff => {
            out.push(0xfd);
            out.extend_from_slice(&(len as u16).to_le_bytes());
        }
        _ => {
            out.push(0xfe);
            out.extend_from_slice(&(len as u32).to_le_bytes());
        }
    }
    out.extend_from_slice(script);
    out
}

/// Independent reconstruction via rust-bitcoin's TaprootBuilder —
/// used by tests as the cross-check, and exported for verifiers
/// that prefer the bitcoin-crate types (control block for the
/// refund script-path comes from here).
pub fn spend_info(
    internal_x: &[u8; 32],
    ta_leaf_hash: &[u8; 32],
    refund_script: &ScriptBuf,
) -> Result<bitcoin::taproot::TaprootSpendInfo, Box<dyn std::error::Error>> {
    let secp = BtcSecp::verification_only();
    let internal = bitcoin::XOnlyPublicKey::from_slice(internal_x)?;
    let info = TaprootBuilder::new()
        .add_hidden_node(1, TapNodeHash::from_byte_array(*ta_leaf_hash))?
        .add_leaf(1, refund_script.clone())?
        .finalize(&secp, internal)
        .map_err(|_| "taproot finalize")?;
    Ok(info)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adaptor::{decrypt, presign, verify_presig};
    use secp256k1::schnorr::Signature;
    use secp256k1::{PublicKey, XOnlyPublicKey};

    fn fixture() -> ([u8; 32], [u8; 32], [u8; 32], ScriptBuf) {
        let sk = tagged_hash("test/funding-internal", b"rail1-funding");
        let secp = Secp256k1::new();
        let (px, _) = SecretKey::from_byte_array(sk)
            .unwrap()
            .x_only_public_key(&secp);
        let ta_leaf = tagged_hash("test/ta-leaf", b"opaque-commitment");
        let refund = refund_leaf_script(
            144,
            &tagged_hash("test/user-x", b"u"),
            &tagged_hash("test/lp-x", b"l"),
        );
        (sk, px.serialize(), ta_leaf, refund)
    }

    #[test]
    fn output_key_matches_independent_taproot_builder() {
        let (_, px, ta_leaf, refund) = fixture();
        let ours = funding_output(&px, &ta_leaf, &refund);
        let info = spend_info(&px, &ta_leaf, &refund).unwrap();
        assert_eq!(
            ours.output_x,
            info.output_key().to_x_only_public_key().serialize(),
            "our TapTweak path must equal rust-bitcoin's TaprootBuilder"
        );
        assert_eq!(
            ours.merkle_root,
            info.merkle_root().unwrap().to_byte_array()
        );
    }

    #[test]
    fn refund_script_path_has_a_valid_control_block() {
        let (_, px, ta_leaf, refund) = fixture();
        let info = spend_info(&px, &ta_leaf, &refund).unwrap();
        let cb = info
            .control_block(&(refund.clone(), LeafVersion::TapScript))
            .expect("refund leaf is in the tree");
        let secp = BtcSecp::verification_only();
        assert!(cb.verify_taproot_commitment(
            &secp,
            info.output_key().to_x_only_public_key(),
            &refund
        ));
    }

    #[test]
    fn cet_keyspend_under_q_via_adaptor() {
        // The construction's point: a CET spends Q by KEY PATH with
        // an adaptor signature — decrypted with the oracle scalar,
        // it must verify as a plain BIP-340 signature under Q.
        let secp = Secp256k1::new();
        let (sk, px, ta_leaf, refund) = fixture();
        let f = funding_output(&px, &ta_leaf, &refund);
        let tweaked = keyspend_secret(&sk, &f.merkle_root).unwrap();

        // Sanity: tweaked secret's pubkey is Q.
        let (qx, _) = SecretKey::from_byte_array(tweaked)
            .unwrap()
            .x_only_public_key(&secp);
        assert_eq!(qx.serialize(), f.output_x);

        // Oracle-style adaptor over the tweaked key.
        let t = SecretKey::from_byte_array(tagged_hash("test/t", b"oracle")).unwrap();
        let t_point: PublicKey = t.public_key(&secp);
        let msg = tagged_hash("test/cet-sighash", b"bucket-3");
        let base = tagged_hash("test/nonce-base", b"cet-3");

        let presig = presign(&tweaked, &base, &msg, &t_point).unwrap();
        assert!(verify_presig(&presig, &f.output_x, &msg, &t_point).unwrap());

        let sig = decrypt(&presig, &t.secret_bytes()).unwrap();
        let q = XOnlyPublicKey::from_byte_array(f.output_x).unwrap();
        secp.verify_schnorr(&Signature::from_byte_array(sig), &msg, &q)
            .expect("CET keyspend signature verifies under the funding output key");
    }

    #[test]
    fn holder_only_refund_leaf_has_a_valid_control_block() {
        // The offline maturity-floor CSV fallback: a holder-only refund
        // leaf must sit in Q's tree with a verifiable control block, so the
        // holder can spend Q via the script path after the CSV.
        let (_, px, ta_leaf, _) = fixture();
        let holder = tagged_hash("test/holder-x", b"h");
        let refund = refund_leaf_holder_only(144, &holder);
        let info = spend_info(&px, &ta_leaf, &refund).unwrap();
        let cb = info
            .control_block(&(refund.clone(), LeafVersion::TapScript))
            .expect("holder-only refund leaf is in the tree");
        let secp = BtcSecp::verification_only();
        assert!(cb.verify_taproot_commitment(
            &secp,
            info.output_key().to_x_only_public_key(),
            &refund
        ));
    }

    #[test]
    fn different_ta_leaf_changes_q() {
        let (_, px, ta_leaf, refund) = fixture();
        let a = funding_output(&px, &ta_leaf, &refund);
        let other = tagged_hash("test/ta-leaf", b"different-commitment");
        let b = funding_output(&px, &other, &refund);
        assert_ne!(a.output_x, b.output_x, "Q binds the TA commitment");
    }
}
