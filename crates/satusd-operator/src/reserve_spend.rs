//! Committee reserve-spend transaction (§11.2 MultisigReserve, M6 pilot).
//!
//! The reserve BTC is held at a real 3-of-5 **P2WSH multisig** address; a finalized
//! claim is reimbursed by building a spend, collecting ≥ threshold committee ECDSA
//! signatures, and broadcasting it. This *realizes* the `ReserveSpend` that
//! `satusd-reserve::MultisigReserve` only models. Interim — BitVM3 supersedes the
//! multisig reserve at M8 (same `OptimisticEnforcementBackend` trait, DL-7/DL-33).

use bitcoin::blockdata::opcodes::all::OP_CHECKMULTISIG;
use bitcoin::ecdsa::Signature as EcdsaSig;
use bitcoin::hashes::Hash;
use bitcoin::secp256k1::{Message, Secp256k1, SecretKey};
use bitcoin::sighash::{EcdsaSighashType, SighashCache};
use bitcoin::{
    absolute::LockTime, script::Builder, transaction::Version, Address, Amount, Network, OutPoint,
    PublicKey, ScriptBuf, Sequence, Transaction, TxIn, TxOut, Witness,
};

/// The `OP_m <pk1>..<pkN> OP_n OP_CHECKMULTISIG` witnessScript. Pubkeys in the
/// given order; signatures must later be supplied in that same order.
pub fn reserve_witness_script(pubkeys: &[PublicKey], threshold: usize) -> ScriptBuf {
    let mut b = Builder::new().push_int(threshold as i64);
    for pk in pubkeys {
        b = b.push_key(pk);
    }
    b.push_int(pubkeys.len() as i64)
        .push_opcode(OP_CHECKMULTISIG)
        .into_script()
}

/// The P2WSH reserve address committing to `witness_script`.
pub fn reserve_address(witness_script: &ScriptBuf, network: Network) -> Address {
    Address::p2wsh(witness_script, network)
}

/// An unsigned spend of `outpoint` paying `amount` sats to `payout_spk`
/// (fee = the funded UTXO value − `amount`, left to the caller to size).
pub fn build_reserve_spend(outpoint: OutPoint, payout_spk: ScriptBuf, amount: u64) -> Transaction {
    Transaction {
        version: Version::TWO,
        lock_time: LockTime::ZERO,
        input: vec![TxIn {
            previous_output: outpoint,
            script_sig: ScriptBuf::new(),
            sequence: Sequence::ENABLE_RBF_NO_LOCKTIME,
            witness: Witness::new(),
        }],
        output: vec![TxOut {
            value: Amount::from_sat(amount),
            script_pubkey: payout_spk,
        }],
    }
}

/// The BIP143 sighash for input 0 of a P2WSH multisig spend.
pub fn reserve_sighash(tx: &Transaction, witness_script: &ScriptBuf, utxo_value: u64) -> [u8; 32] {
    SighashCache::new(tx)
        .p2wsh_signature_hash(
            0,
            witness_script,
            Amount::from_sat(utxo_value),
            EcdsaSighashType::All,
        )
        .expect("p2wsh sighash")
        .to_byte_array()
}

/// Sign input 0 with `signers` (each `(index_in_pubkeys, secret_key)`), filling the
/// P2WSH multisig witness `<dummy> <sig...> <witnessScript>`. CHECKMULTISIG requires
/// the signatures in the same order as their pubkeys in the script, so signers are
/// sorted by index; supply at least `threshold` of them.
pub fn sign_reserve_spend(
    tx: &mut Transaction,
    witness_script: &ScriptBuf,
    utxo_value: u64,
    mut signers: Vec<(usize, SecretKey)>,
) {
    let secp = Secp256k1::new();
    let msg = Message::from_digest(reserve_sighash(tx, witness_script, utxo_value));
    signers.sort_by_key(|(i, _)| *i);

    let mut w = Witness::new();
    let dummy: &[u8] = &[]; // CHECKMULTISIG off-by-one: an extra stack element
    w.push(dummy);
    for (_, sk) in &signers {
        let sig = EcdsaSig {
            signature: secp.sign_ecdsa(&msg, sk),
            sighash_type: EcdsaSighashType::All,
        };
        w.push(sig.to_vec());
    }
    w.push(witness_script.as_bytes());
    tx.input[0].witness = w;
}

#[cfg(test)]
mod tests {
    use super::*;
    use bitcoin::secp256k1::Secp256k1;

    fn key(seed: u8) -> (SecretKey, PublicKey) {
        let secp = Secp256k1::new();
        let sk = SecretKey::from_slice(&[seed; 32]).unwrap();
        (sk, PublicKey::new(sk.public_key(&secp)))
    }

    #[test]
    fn build_sign_3of5_witness_is_well_formed_and_sigs_verify() {
        let secp = Secp256k1::new();
        let keys: Vec<_> = (1..=5u8).map(key).collect();
        let pubkeys: Vec<PublicKey> = keys.iter().map(|(_, pk)| *pk).collect();
        let ws = reserve_witness_script(&pubkeys, 3);
        let addr = reserve_address(&ws, Network::Regtest);
        assert!(addr.to_string().starts_with("bcrt1")); // regtest bech32 P2WSH

        let outpoint = OutPoint {
            txid: "aa".repeat(32).parse().unwrap(),
            vout: 0,
        };
        let utxo_value = 1_000_000;
        let payout = ScriptBuf::from_hex("0014").unwrap_or_else(|_| addr.script_pubkey());
        let mut tx = build_reserve_spend(outpoint, payout, utxo_value - 500);

        // Sign with members 0, 2, 4 (deliberately out of order → must get re-sorted).
        let signers = vec![(4, keys[4].0), (0, keys[0].0), (2, keys[2].0)];
        sign_reserve_spend(&mut tx, &ws, utxo_value, signers);

        // Witness: [dummy, sig, sig, sig, witnessScript].
        let w = &tx.input[0].witness;
        assert_eq!(w.len(), 5);
        assert!(w.nth(0).unwrap().is_empty());
        assert_eq!(w.nth(4).unwrap(), ws.as_bytes());

        // Each signature verifies against its committee pubkey + the sighash, in
        // ascending pubkey order (0, 2, 4) — what CHECKMULTISIG will check.
        let msg = Message::from_digest(reserve_sighash(&tx, &ws, utxo_value));
        for (witness_pos, key_idx) in [(1usize, 0usize), (2, 2), (3, 4)] {
            let raw = w.nth(witness_pos).unwrap();
            let sig = bitcoin::ecdsa::Signature::from_slice(raw).unwrap();
            assert!(secp
                .verify_ecdsa(&msg, &sig.signature, &pubkeys[key_idx].inner)
                .is_ok());
        }
    }

    #[test]
    fn address_is_deterministic_in_pubkey_order() {
        let keys: Vec<_> = (1..=5u8).map(key).collect();
        let pks: Vec<PublicKey> = keys.iter().map(|(_, pk)| *pk).collect();
        let a = reserve_address(&reserve_witness_script(&pks, 3), Network::Regtest);
        let b = reserve_address(&reserve_witness_script(&pks, 3), Network::Regtest);
        assert_eq!(a, b);
        // A different threshold ⇒ a different address.
        let c = reserve_address(&reserve_witness_script(&pks, 4), Network::Regtest);
        assert_ne!(a, c);
    }
}
