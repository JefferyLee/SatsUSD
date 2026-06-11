//! Event identity, digit decomposition, and deterministic nonce
//! derivation (spec 03 §3.1/§3.3).

use satusd_rail::encode::tagged_hash;

pub const NONCE_TAG: &str = "SatUSD/oracle/nonce/v1";

/// Spec 03 §3.1 defaults: base-2, 20 digits, whole USD per BTC.
pub const BASE: u16 = 2;
pub const NB_DIGITS: u16 = 20;
pub const MAX_OUTCOME: u32 = (1 << NB_DIGITS) - 1;

/// `event_id = "SatUSD/BTCUSD/" ‖ decimal(unix_ts)`.
pub fn event_id(unix_ts: u64) -> String {
    format!("SatUSD/BTCUSD/{unix_ts}")
}

/// Decompose a whole-USD price into base-2 digits, most significant
/// first. Overflow clamps to the maximal value (all ones), per
/// dlcspecs and spec 03 §3.1.
pub fn digits(price_usd: u32) -> [u8; NB_DIGITS as usize] {
    let clamped = price_usd.min(MAX_OUTCOME);
    let mut out = [0u8; NB_DIGITS as usize];
    for (i, d) in out.iter_mut().enumerate() {
        *d = ((clamped >> (NB_DIGITS as usize - 1 - i)) & 1) as u8;
    }
    out
}

/// Recompose digits into the price (for verifiers).
pub fn price_from_digits(digits: &[u8]) -> u32 {
    digits
        .iter()
        .fold(0u32, |acc, d| (acc << 1) | u32::from(*d))
}

/// The attestation message for one digit outcome:
/// `tagged_hash("DLC/oracle/attestation/v0", outcome_utf8)` —
/// outcomes are the single-character strings "0"/"1" (NFC-trivial).
pub fn attestation_msg(digit: u8) -> [u8; 32] {
    let s = if digit == 0 { "0" } else { "1" };
    tagged_hash("DLC/oracle/attestation/v0", s.as_bytes())
}

/// Deterministic per-event, per-digit nonce:
/// `k = tagged_hash(NONCE_TAG, seed ‖ event_id ‖ u16_be(digit_index))`.
///
/// Spec 03 §3.3 refinement: the derivation MUST include the digit
/// index — one nonce per digit per event, never reused.
pub fn nonce(seed: &[u8; 32], event_id: &str, digit_index: u16) -> [u8; 32] {
    let mut buf = Vec::with_capacity(32 + event_id.len() + 2);
    buf.extend_from_slice(seed);
    buf.extend_from_slice(event_id.as_bytes());
    buf.extend_from_slice(&digit_index.to_be_bytes());
    tagged_hash(NONCE_TAG, &buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digit_roundtrip_and_clamp() {
        for p in [0u32, 1, 60_123, MAX_OUTCOME] {
            assert_eq!(price_from_digits(&digits(p)), p);
        }
        assert_eq!(
            price_from_digits(&digits(u32::MAX)),
            MAX_OUTCOME,
            "overflow clamps"
        );
        // 60123 = 0b1110_1011_1101_1011 → check MSB-first layout.
        let d = digits(60_123);
        assert_eq!(d[..4], [0, 0, 0, 0], "top digits zero below 2^16");
        assert_eq!(d[4], 1);
    }

    #[test]
    fn nonces_unique_per_event_and_digit() {
        let seed = [7u8; 32];
        let a = nonce(&seed, &event_id(1_700_000_000), 0);
        let b = nonce(&seed, &event_id(1_700_000_000), 1);
        let c = nonce(&seed, &event_id(1_700_000_001), 0);
        assert_ne!(a, b);
        assert_ne!(a, c);
        assert_eq!(
            a,
            nonce(&seed, &event_id(1_700_000_000), 0),
            "deterministic"
        );
    }
}
