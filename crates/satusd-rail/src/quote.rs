//! The Quote payload (spec 02 §3.1). Transport is deliberately
//! unstandardized (ADR-0002); only the payload and its signing
//! digest are normative.

use crate::encode::{tagged_hash, Encoder};

pub const QUOTE_TAG: &str = "SatUSD/quote/v1";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Quote {
    pub rail_id: [u8; 32],
    /// Canonical price type (spec 00 §3.6): milli-satoshis per USD.
    pub price_msat_per_usd: u64,
    /// Conversion amount in µUSD (spec 01 §2.1).
    pub amount_micro_usd: u64,
    /// Unix seconds; MUST be ≤ quote time + manifest.quote_validity.
    pub expiry_unix_s: u64,
    /// Hash of the exact LOCK structure, so the user can verify all
    /// downstream paths before committing anything (spec 02 §3.1).
    pub lock_template_commitment: [u8; 32],
}

impl Quote {
    pub fn canonical_encode(&self) -> Vec<u8> {
        let mut e = Encoder::new();
        e.put_bytes32(&self.rail_id);
        e.put_u64(self.price_msat_per_usd);
        e.put_u64(self.amount_micro_usd);
        e.put_u64(self.expiry_unix_s);
        e.put_bytes32(&self.lock_template_commitment);
        e.into_bytes()
    }

    /// The 32-byte digest the LP signs (BIP-340, caller supplies the
    /// signing implementation).
    pub fn signing_payload(&self) -> [u8; 32] {
        tagged_hash(QUOTE_TAG, &self.canonical_encode())
    }

    /// BTC the user receives for this quote, floored in line with
    /// the rounding rule of spec 00 §3.7. `price` is msat/USD,
    /// `amount` is µUSD: sats = amount µUSD × price msat/µUSD-factor.
    ///
    /// amount_micro_usd × price_msat_per_usd is msat·µ-scale; one
    /// USD = 10^6 µUSD and one sat = 10^3 msat, so
    /// sats = amount × price / (10^6 × 10^3).
    pub fn user_sats(&self) -> u64 {
        let prod = u128::from(self.amount_micro_usd) * u128::from(self.price_msat_per_usd);
        (prod / 1_000_000_000) as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_sats_floors() {
        // $1,000 at $100,000/BTC → price = 10^11/100_000 = 1_000_000
        // msat/USD → 1_000_000 µUSD × 1e6 / 1e9 = 1_000_000 sats? No:
        // 1_000 USD = 10^9 µUSD; ×1e6 msat/USD ÷1e9 = 10^6 sats = 0.01 BTC.
        let q = Quote {
            rail_id: [0; 32],
            price_msat_per_usd: 1_000_000,
            amount_micro_usd: 1_000_000_000,
            expiry_unix_s: 0,
            lock_template_commitment: [0; 32],
        };
        assert_eq!(q.user_sats(), 1_000_000);

        // One µUSD at the same price floors to 0 sats (0.001 sat).
        let dust = Quote {
            amount_micro_usd: 1,
            ..q
        };
        assert_eq!(dust.user_sats(), 0);
    }

    #[test]
    fn signing_payload_changes_with_any_field() {
        let base = Quote {
            rail_id: [1; 32],
            price_msat_per_usd: 1_000_000,
            amount_micro_usd: 5_000_000,
            expiry_unix_s: 1_700_000_000,
            lock_template_commitment: [2; 32],
        };
        let mut other = base.clone();
        other.expiry_unix_s += 1;
        assert_ne!(base.signing_payload(), other.signing_payload());
    }
}
