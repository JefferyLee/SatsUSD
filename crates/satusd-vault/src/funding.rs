//! The vault funding output (spec 06 §2): one P2TR output holding the
//! minter's BTC collateral, co-controlled by minter + reserve.
//!
//! ```text
//! internal key  P  = MuSig2_KeyAgg(minter, reserve)   (v0: single key)
//! script tree   T  = { refund_leaf }                  (single leaf)
//! output key    Q  = P + TapTweak(P ‖ MerkleRoot(T))·G
//! spending      key-path: checkpoint/maturity CET via adaptor (06 §4–5),
//!                         burn-reclaim co-signed by minter+reserve
//!               script-path: refund_leaf after the CSV tlock backstop (06 §6)
//! ```
//!
//! Unlike the rail-1 funding output ({ta_leaf, refund_leaf}, two
//! leaves), the vault tree is a single refund leaf: the collateral is
//! pure BTC and the minted SatUSD is issued separately, so no TA
//! commitment rides the output. A single-leaf tree's BIP-341 merkle
//! root is the leaf hash itself.
//!
//! The refund leaf and the keyspend-secret tweak are reused verbatim
//! from `satusd_rail1::funding`; CETs are key-path spends of Q fed to
//! `satusd_rail1::adaptor::presign`.

use bitcoin::hashes::Hash;
use bitcoin::key::Secp256k1 as BtcSecp;
use bitcoin::taproot::{LeafVersion, TapLeafHash, TaprootBuilder};
use bitcoin::ScriptBuf;
use satusd_crypto::nums::tap_tweak_data;

pub use satusd_rail1::funding::{keyspend_secret, refund_leaf_script, sibling_preimage};

/// The vault funding output's tree facts, reconstructable by any
/// verifier from (internal key, refund script).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VaultFundingOutput {
    /// x-only internal key (MuSig2(minter, reserve) aggregate; v0 single).
    pub internal_x: [u8; 32],
    /// BIP-341 merkle root over the single-leaf tree {refund_leaf} —
    /// equal to the refund leaf hash.
    pub merkle_root: [u8; 32],
    /// x-only output key Q — what the on-chain scriptPubKey carries.
    pub output_x: [u8; 32],
}

/// Build the vault funding output facts from the internal key and the
/// refund script. The tree is a single leaf, so its merkle root is the
/// refund leaf hash.
pub fn vault_funding_output(internal_x: &[u8; 32], refund_script: &ScriptBuf) -> VaultFundingOutput {
    let leaf = TapLeafHash::from_script(refund_script, LeafVersion::TapScript);
    let root = leaf.to_byte_array();
    VaultFundingOutput {
        internal_x: *internal_x,
        merkle_root: root,
        output_x: tap_tweak_data(internal_x, &root),
    }
}

/// Independent reconstruction via rust-bitcoin's TaprootBuilder — the
/// test cross-check, and the source of the refund-leaf control block
/// for the script-path (tlock) spend.
pub fn spend_info(
    internal_x: &[u8; 32],
    refund_script: &ScriptBuf,
) -> Result<bitcoin::taproot::TaprootSpendInfo, Box<dyn std::error::Error>> {
    let secp = BtcSecp::verification_only();
    let internal = bitcoin::XOnlyPublicKey::from_slice(internal_x)?;
    let info = TaprootBuilder::new()
        .add_leaf(0, refund_script.clone())?
        .finalize(&secp, internal)
        .map_err(|_| "taproot finalize")?;
    Ok(info)
}

#[cfg(test)]
mod tests {
    use super::*;
    use satusd_rail::encode::tagged_hash;
    use satusd_rail1::adaptor::{decrypt, presign, verify_presig};
    use secp256k1::schnorr::Signature;
    use secp256k1::{PublicKey, Secp256k1, SecretKey, XOnlyPublicKey};

    fn fixture() -> ([u8; 32], [u8; 32], ScriptBuf) {
        let sk = tagged_hash("test/vault-internal", b"vault-funding");
        let secp = Secp256k1::new();
        let (px, _) = SecretKey::from_byte_array(sk)
            .unwrap()
            .x_only_public_key(&secp);
        // refund_leaf = <CSV tlock> 2-of-2(minter, reserve)
        let refund = refund_leaf_script(
            4032,
            &tagged_hash("test/minter-x", b"m"),
            &tagged_hash("test/reserve-x", b"r"),
        );
        (sk, px.serialize(), refund)
    }

    #[test]
    fn output_key_matches_independent_taproot_builder() {
        let (_, px, refund) = fixture();
        let ours = vault_funding_output(&px, &refund);
        let info = spend_info(&px, &refund).unwrap();
        assert_eq!(
            ours.output_x,
            info.output_key().to_x_only_public_key().serialize(),
            "single-leaf TapTweak path must equal rust-bitcoin's TaprootBuilder"
        );
        assert_eq!(
            ours.merkle_root,
            info.merkle_root().unwrap().to_byte_array(),
            "single-leaf merkle root is the refund leaf hash"
        );
    }

    #[test]
    fn refund_script_path_has_a_valid_control_block() {
        let (_, px, refund) = fixture();
        let info = spend_info(&px, &refund).unwrap();
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
    fn checkpoint_cet_keyspend_under_q_via_adaptor() {
        // A checkpoint CET spends Q by KEY PATH with an adaptor
        // signature anticipating the oracle; decrypted with the oracle
        // scalar it must verify as a plain BIP-340 signature under Q.
        let secp = Secp256k1::new();
        let (sk, px, refund) = fixture();
        let f = vault_funding_output(&px, &refund);
        let tweaked = keyspend_secret(&sk, &f.merkle_root).unwrap();

        // The tweaked secret's pubkey is Q.
        let (qx, _) = SecretKey::from_byte_array(tweaked)
            .unwrap()
            .x_only_public_key(&secp);
        assert_eq!(qx.serialize(), f.output_x);

        // Oracle-anticipated adaptor over the tweaked (output) key.
        let t = SecretKey::from_byte_array(tagged_hash("test/t", b"oracle-crash")).unwrap();
        let t_point: PublicKey = t.public_key(&secp);
        let msg = tagged_hash("test/cet-sighash", b"checkpoint-crash-bucket");
        let base = tagged_hash("test/nonce-base", b"vault-cet");

        let presig = presign(&tweaked, &base, &msg, &t_point).unwrap();
        assert!(verify_presig(&presig, &f.output_x, &msg, &t_point).unwrap());

        let sig = decrypt(&presig, &t.secret_bytes()).unwrap();
        let q = XOnlyPublicKey::from_byte_array(f.output_x).unwrap();
        secp.verify_schnorr(&Signature::from_byte_array(sig), &msg, &q)
            .expect("checkpoint CET keyspend verifies under the vault output key");
    }

    #[test]
    fn different_refund_changes_q() {
        let (_, px, refund) = fixture();
        let a = vault_funding_output(&px, &refund);
        let other = refund_leaf_script(
            288,
            &tagged_hash("test/minter-x", b"m"),
            &tagged_hash("test/reserve-x", b"r"),
        );
        let b = vault_funding_output(&px, &other);
        assert_ne!(a.output_x, b.output_x, "Q binds the refund terms (CSV tlock)");
    }
}
