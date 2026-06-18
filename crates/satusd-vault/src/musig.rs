//! MuSig2(minter, reserve) for the vault funding output (spec 06 §2).
//!
//! The end-state funding output's internal key is the MuSig2 aggregate
//! of the minter and reserve keys (BIP-327), tweaked by the refund-leaf
//! merkle root to Q. The crash/reclaim key-path spends are then real
//! 2-of-2 co-signatures, not v0's single funding key. The `secp256k1`
//! crate (0.31) still ships no musig module, so this wraps the audited
//! `musig2` crate — which pins its own `secp256k1`, hence everything
//! crosses the boundary as bytes (pubkeys 33B, secrets/x-only/roots
//! 32B, signatures 64B), independently verifiable under the project's
//! own secp256k1.

use musig2::secp::{MaybePoint, MaybeScalar};
use musig2::secp256k1::{PublicKey, Secp256k1, SecretKey, XOnlyPublicKey};
use musig2::{
    AdaptorSignature, BinaryEncoding, CompactSignature, FirstRound, KeyAggContext, LiftedSignature,
    PartialSignature, SecNonceSpices,
};
use satusd_rail::encode::tagged_hash;

fn pubkey(sk: &[u8; 32]) -> PublicKey {
    let secp = Secp256k1::new();
    SecretKey::from_byte_array(*sk).expect("sk").public_key(&secp)
}

/// KeyAggContext over [minter, reserve], optionally taproot-tweaked.
/// Order is fixed (no KeySort) so signer index 0 = minter, 1 = reserve.
fn ctx(minter_sk: &[u8; 32], reserve_sk: &[u8; 32], tweak: Option<&[u8; 32]>) -> KeyAggContext {
    let c = KeyAggContext::new([pubkey(minter_sk), pubkey(reserve_sk)]).expect("keyagg");
    match tweak {
        Some(root) => c.with_taproot_tweak(root).expect("taproot tweak"),
        None => c,
    }
}

/// The UNTWEAKED MuSig2 aggregate internal key (x-only) — what the vault
/// funding output uses as `internal_x` (P = KeyAgg(minter, reserve)).
pub fn aggregate_internal_x(minter_sk: &[u8; 32], reserve_sk: &[u8; 32]) -> [u8; 32] {
    ctx(minter_sk, reserve_sk, None)
        .aggregated_pubkey::<XOnlyPublicKey>()
        .serialize()
}

/// The taproot output key Q = taptweak(P_agg, merkle_root) computed via
/// MuSig2 — must equal the project's `vault_funding_output`.
pub fn aggregate_q(minter_sk: &[u8; 32], reserve_sk: &[u8; 32], merkle_root: &[u8; 32]) -> [u8; 32] {
    ctx(minter_sk, reserve_sk, Some(merkle_root))
        .aggregated_pubkey::<XOnlyPublicKey>()
        .serialize()
}

/// A real 2-of-2 MuSig2 key-path signature under Q over `message`: the
/// minter and reserve each contribute a nonce and a partial signature,
/// aggregated to one BIP-340 signature. Both rounds are run locally
/// here (v0 single-process); production exchanges the public nonces and
/// partial signatures between the two parties. Returns the 64-byte sig.
pub fn cosign_keyspend(
    minter_sk: &[u8; 32],
    reserve_sk: &[u8; 32],
    merkle_root: &[u8; 32],
    message: &[u8; 32],
) -> [u8; 64] {
    let msk = SecretKey::from_byte_array(*minter_sk).expect("minter sk");
    let rsk = SecretKey::from_byte_array(*reserve_sk).expect("reserve sk");

    // Per-signer nonce seeds, bound to (root, message) so a nonce is
    // never reused across messages.
    let mut mat = merkle_root.to_vec();
    mat.extend_from_slice(message);
    let seed_m = tagged_hash("SatUSD/vault/musig-nonce/minter/v1", &mat);
    let seed_r = tagged_hash("SatUSD/vault/musig-nonce/reserve/v1", &mat);

    // Round 1: each signer's public nonce (separate ctx instances —
    // FirstRound::new consumes the ctx).
    let mut fr_m = FirstRound::new(
        ctx(minter_sk, reserve_sk, Some(merkle_root)),
        seed_m,
        0,
        SecNonceSpices::new().with_seckey(msk),
    )
    .expect("first round minter");
    let mut fr_r = FirstRound::new(
        ctx(minter_sk, reserve_sk, Some(merkle_root)),
        seed_r,
        1,
        SecNonceSpices::new().with_seckey(rsk),
    )
    .expect("first round reserve");
    let nonce_m = fr_m.our_public_nonce();
    let nonce_r = fr_r.our_public_nonce();
    fr_m.receive_nonce(1, nonce_r).expect("minter receives reserve nonce");
    fr_r.receive_nonce(0, nonce_m).expect("reserve receives minter nonce");

    // Round 2: partial signatures, then aggregate.
    let mut sr_m = fr_m.finalize(msk, *message).expect("second round minter");
    let sr_r = fr_r.finalize(rsk, *message).expect("second round reserve");
    let partial_r: PartialSignature = sr_r.our_signature();
    sr_m.receive_signature(1, partial_r).expect("minter receives reserve partial");
    let sig: CompactSignature = sr_m.finalize().expect("aggregate signature");
    sig.serialize()
}

/// A 2-of-2 MuSig2 *adaptor* key-path signature under Q over `message`,
/// verifiably encrypted to the oracle anticipation point `adaptor_point`
/// (`T`, 33-byte compressed `t·G`). The minter and reserve each contribute
/// a nonce and a partial adaptor signature; `receive_signature` verifies
/// the counterparty's partial before aggregation, so a tampered partial is
/// rejected here (S1). Neither party can produce this alone — this is the
/// maturity CET pre-signed at issuance that closes v0's single-key custody
/// gap. Decrypt at maturity with the oracle scalar via [`adapt_keyspend`].
/// Returns the 65-byte adaptor signature (compressed `R` ‖ `s`).
pub fn cosign_keyspend_adaptor(
    minter_sk: &[u8; 32],
    reserve_sk: &[u8; 32],
    merkle_root: &[u8; 32],
    message: &[u8; 32],
    adaptor_point: &[u8; 33],
) -> [u8; 65] {
    let msk = SecretKey::from_byte_array(*minter_sk).expect("minter sk");
    let rsk = SecretKey::from_byte_array(*reserve_sk).expect("reserve sk");
    let t = MaybePoint::try_from(&adaptor_point[..]).expect("adaptor point");

    // Nonce seeds bound to (root, message, adaptor point) — never reused.
    let mut mat = merkle_root.to_vec();
    mat.extend_from_slice(message);
    mat.extend_from_slice(adaptor_point);
    let seed_m = tagged_hash("SatUSD/vault/musig-adaptor-nonce/minter/v1", &mat);
    let seed_r = tagged_hash("SatUSD/vault/musig-adaptor-nonce/reserve/v1", &mat);

    let mut fr_m = FirstRound::new(
        ctx(minter_sk, reserve_sk, Some(merkle_root)),
        seed_m,
        0,
        SecNonceSpices::new().with_seckey(msk),
    )
    .expect("first round minter");
    let mut fr_r = FirstRound::new(
        ctx(minter_sk, reserve_sk, Some(merkle_root)),
        seed_r,
        1,
        SecNonceSpices::new().with_seckey(rsk),
    )
    .expect("first round reserve");
    let nonce_m = fr_m.our_public_nonce();
    let nonce_r = fr_r.our_public_nonce();
    fr_m.receive_nonce(1, nonce_r).expect("minter receives reserve nonce");
    fr_r.receive_nonce(0, nonce_m).expect("reserve receives minter nonce");

    let mut sr_m = fr_m.finalize_adaptor(msk, t, *message).expect("second round minter");
    let sr_r = fr_r.finalize_adaptor(rsk, t, *message).expect("second round reserve");
    let partial_r: PartialSignature = sr_r.our_signature();
    sr_m.receive_signature(1, partial_r).expect("minter receives reserve partial");
    let adaptor = sr_m
        .finalize_adaptor::<AdaptorSignature>()
        .expect("aggregate adaptor signature");
    adaptor.to_bytes()
}

/// Adapt a 2-of-2 adaptor key-path signature (from
/// [`cosign_keyspend_adaptor`]) with the oracle scalar `oracle_secret`
/// (the discrete log of the anticipation point) into a standard 64-byte
/// BIP-340 key-path signature under Q. Anyone holding the public oracle
/// attestation can do this — no signer key needed — preserving unilateral
/// broadcast and the offline maturity floor.
pub fn adapt_keyspend(adaptor_sig: &[u8; 65], oracle_secret: &[u8; 32]) -> [u8; 64] {
    let adaptor = AdaptorSignature::from_bytes(&adaptor_sig[..]).expect("adaptor sig bytes");
    let t = MaybeScalar::try_from(&oracle_secret[..]).expect("oracle secret");
    let lifted: LiftedSignature = adaptor.adapt(t).expect("adapt");
    lifted.to_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::funding::{refund_leaf_script, vault_funding_output};
    use satusd_rail::encode::tagged_hash as th;

    fn keys() -> ([u8; 32], [u8; 32]) {
        (th("test/musig-minter", b"m"), th("test/musig-reserve", b"r"))
    }

    #[test]
    fn musig_aggregate_matches_project_taptweak() {
        let (mk, rs) = keys();
        let refund = refund_leaf_script(4032, &th("test/m-x", b"m"), &th("test/r-x", b"r"));
        // Untweaked aggregate feeds vault_funding_output as internal_x;
        // its tweaked Q must equal MuSig2's taproot-tweaked aggregate.
        let internal_x = aggregate_internal_x(&mk, &rs);
        let f = vault_funding_output(&internal_x, &refund);
        let q_musig = aggregate_q(&mk, &rs, &f.merkle_root);
        assert_eq!(
            f.output_x, q_musig,
            "project tap_tweak(P_agg, root) must equal MuSig2's taproot tweak"
        );
    }

    #[test]
    fn cosigned_keyspend_verifies_under_q() {
        // The 2-of-2 MuSig2 signature must verify as a plain BIP-340
        // keyspend under Q — checked with the PROJECT's secp256k1 (an
        // independent implementation from musig2's pinned one).
        let (mk, rs) = keys();
        let refund = refund_leaf_script(4032, &th("test/m-x", b"m"), &th("test/r-x", b"r"));
        let internal_x = aggregate_internal_x(&mk, &rs);
        let f = vault_funding_output(&internal_x, &refund);
        let msg = th("test/keyspend-sighash", b"vault-reclaim");

        let sig = cosign_keyspend(&mk, &rs, &f.merkle_root, &msg);

        let secp = secp256k1::Secp256k1::verification_only();
        let q = secp256k1::XOnlyPublicKey::from_byte_array(f.output_x).unwrap();
        secp.verify_schnorr(&secp256k1::schnorr::Signature::from_byte_array(sig), &msg, &q)
            .expect("2-of-2 MuSig2 keyspend verifies under Q");
    }

    #[test]
    fn cosigned_adaptor_keyspend_decrypts_to_valid_q_sig() {
        // The 2-of-2 MuSig2 ADAPTOR keyspend, pre-signed by both parties at
        // issuance and encrypted to the oracle point T = t·G, must — once
        // adapted with the published scalar t — verify as a plain BIP-340
        // keyspend under Q, checked with the PROJECT's secp256k1.
        let (mk, rs) = keys();
        let refund = refund_leaf_script(4032, &th("test/m-x", b"m"), &th("test/r-x", b"r"));
        let internal_x = aggregate_internal_x(&mk, &rs);
        let f = vault_funding_output(&internal_x, &refund);
        let msg = th("test/adaptor-sighash", b"maturity-cet");

        // Oracle anticipation point T = t·G (project secp256k1).
        let secp = secp256k1::Secp256k1::new();
        let t = th("test/oracle-secret", b"price-bucket");
        let t_point = secp256k1::SecretKey::from_byte_array(t)
            .unwrap()
            .public_key(&secp)
            .serialize();

        let adaptor = cosign_keyspend_adaptor(&mk, &rs, &f.merkle_root, &msg, &t_point);
        let sig = adapt_keyspend(&adaptor, &t);

        let q = secp256k1::XOnlyPublicKey::from_byte_array(f.output_x).unwrap();
        let vsecp = secp256k1::Secp256k1::verification_only();
        vsecp
            .verify_schnorr(&secp256k1::schnorr::Signature::from_byte_array(sig), &msg, &q)
            .expect("adapted 2-of-2 MuSig2 keyspend verifies under Q");

        // Wrong oracle scalar must NOT yield a valid keyspend (the lock holds).
        let wrong = th("test/oracle-secret", b"not-the-bucket");
        let bad = adapt_keyspend(&adaptor, &wrong);
        assert!(vsecp
            .verify_schnorr(&secp256k1::schnorr::Signature::from_byte_array(bad), &msg, &q)
            .is_err());
    }
}
