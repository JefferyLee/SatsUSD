//! NUMS keys and Taproot tweaks for the protocol burn sink (PRD §5.D16, §18.7).
//! ADR-0014 fixes the `TapTweak` primitive and the derivations.

use satusd_types::domain;
use satusd_types::hash::sha256;
use secp256k1::{Scalar, Secp256k1, XOnlyPublicKey};

/// True iff `x` is a valid x-only public key (an even-Y point lifts from it).
pub fn is_valid_xonly(x: &[u8; 32]) -> bool {
    XOnlyPublicKey::from_byte_array(*x).is_ok()
}

/// `derive_nums_key` (§18.7): smallest counter making `SHA256(domain || salt ||
/// counter_be32)` a valid x-only pubkey. Returns that 32-byte x-only key.
pub fn derive_nums_key(domain_name: &str, salt: &[u8]) -> [u8; 32] {
    let mut ctr: u32 = 0;
    loop {
        let seed = sha256(&[domain_name.as_bytes(), salt, &ctr.to_be_bytes()]);
        if is_valid_xonly(&seed) {
            return seed;
        }
        ctr += 1;
    }
}

/// BIP341 tagged hash `SHA256(SHA256(tag) || SHA256(tag) || msg)`.
fn tagged_hash(tag: &str, msg: &[u8]) -> [u8; 32] {
    let th = sha256(&[tag.as_bytes()]);
    sha256(&[&th, &th, msg])
}

/// `TapTweak(internal, tweak)` (ADR-0014): the BIP341 output key
/// `internal + tagged_hash("TapTweak", internal_xonly || tweak) * G`, x-only.
pub fn tap_tweak(internal_xonly: &[u8; 32], tweak: &[u8; 32]) -> [u8; 32] {
    let secp = Secp256k1::verification_only();
    let internal = XOnlyPublicKey::from_byte_array(*internal_xonly)
        .expect("internal key must be valid x-only");
    let mut msg = Vec::with_capacity(64);
    msg.extend_from_slice(internal_xonly);
    msg.extend_from_slice(tweak);
    let t = Scalar::from_be_bytes(tagged_hash("TapTweak", &msg)).expect("tweak scalar in range");
    let (tweaked, _parity) = internal.add_tweak(&secp, &t).expect("taproot tweak");
    tweaked.serialize()
}

/// NUMS internal key for the burn sink (§18.7), salted by `asset_family_id`.
pub fn protocol_burn_internal_key(asset_family_id: &[u8; 32]) -> [u8; 32] {
    derive_nums_key(domain::BURN_SINK, asset_family_id)
}

/// Burn tweak `SHA256("SATUSD_BURN_TWEAK_V1" || asset_family_id)` (§18.7).
pub fn protocol_burn_tweak(asset_family_id: &[u8; 32]) -> [u8; 32] {
    sha256(&[domain::BURN_TWEAK.as_bytes(), asset_family_id])
}

/// `protocol_sink_script_key = TapTweak(burn_internal_key, burn_tweak)` (§5.D16).
pub fn protocol_sink_script_key(asset_family_id: &[u8; 32]) -> [u8; 32] {
    let internal = protocol_burn_internal_key(asset_family_id);
    let tweak = protocol_burn_tweak(asset_family_id);
    tap_tweak(&internal, &tweak)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nums_key_is_valid_and_deterministic() {
        let salt = [0xab; 32];
        let k1 = derive_nums_key(domain::BURN_SINK, &salt);
        let k2 = derive_nums_key(domain::BURN_SINK, &salt);
        assert_eq!(k1, k2);
        assert!(is_valid_xonly(&k1));
    }

    #[test]
    fn sink_derivation_runs_and_prints() {
        let afid = [0x11; 32];
        let ik = protocol_burn_internal_key(&afid);
        let sink = protocol_sink_script_key(&afid);
        assert!(is_valid_xonly(&ik));
        assert!(is_valid_xonly(&sink));
        println!("internal = 0x{}", hex::encode(ik));
        println!("sink     = 0x{}", hex::encode(sink));
    }
}
