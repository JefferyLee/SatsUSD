//! Cross-language interop smoke: verify an EdDSA-BabyJub signature produced by
//! the Node oracle signer (`tools/oracle-signer/`) with `satusd-crypto`'s
//! `babyjubjub-rs` verifier. Recomputes `oracle_message_hash(...)` from the
//! same inputs the daemon signed; exits 0 iff the signature is valid.
//!
//!   eddsa_verify <set_epoch> <price_epoch> <ts_ms> <price_e8> \
//!                <signer_pubkey_hex_32B> <signature_hex_64B>

use std::process::ExitCode;

use satusd_crypto::eddsa::verify_eddsa_babyjub;
use satusd_crypto::poseidon::oracle_message_hash;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 7 {
        eprintln!(
            "usage: {} <set_epoch> <price_epoch> <ts_ms> <price_e8> <pubkey_hex> <sig_hex>",
            args[0]
        );
        return ExitCode::from(2);
    }
    let set_epoch: u64 = args[1].parse().expect("set_epoch u64");
    let price_epoch: u64 = args[2].parse().expect("price_epoch u64");
    let ts_ms: u64 = args[3].parse().expect("ts_ms u64");
    let price_e8: u64 = args[4].parse().expect("price_e8 u64");
    let pubkey: [u8; 32] = hex::decode(&args[5])
        .expect("pubkey hex")
        .try_into()
        .expect("pubkey 32 bytes");
    let sig: [u8; 64] = hex::decode(&args[6])
        .expect("sig hex")
        .try_into()
        .expect("sig 64 bytes");
    let msg = oracle_message_hash(set_epoch, price_epoch, ts_ms, price_e8);
    let ok = verify_eddsa_babyjub(&pubkey, &msg, &sig);
    println!("verify: {}", if ok { "OK" } else { "FAIL" });
    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}
