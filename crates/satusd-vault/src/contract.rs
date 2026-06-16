//! Vault contract terms, the canonical `vault_id`, and the
//! collateral-ratio arithmetic (spec 06 §2–3, §7).
//!
//! Units: collateral in sats; face/mint in µUSD (1 SatUSD = 1e6 µUSD,
//! spec 01); price in whole USD per BTC (spec 03). All CR math is
//! integer (u128 intermediate), rounding always toward the reserve
//! (spec 00 §3.7).

use satusd_rail::encode::tagged_hash;

const SATS_PER_BTC: u128 = 100_000_000;
const MICRO_PER_USD: u128 = 1_000_000;

/// A vault's economic + schedule terms. `vault_id` commits to these;
/// a parameter change is a new vault_id, never a retroactive edit
/// (spec 04 §5, spec 06 §2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VaultTerms {
    /// BTC collateral locked in the funding output.
    pub collateral_sats: u64,
    /// Face value minted, µUSD.
    pub mint_micro_usd: u64,
    /// Minimum opening CR, basis points (spec 06 §8, e.g. 15000).
    pub opening_cr_bps: u32,
    /// Liquidation CR, basis points (e.g. 11000) — crash buckets are
    /// prices where the vault's own CR ≤ this.
    pub liq_cr_bps: u32,
    /// Checkpoint interval, blocks (spec 06 §8 `N`).
    pub checkpoint_interval: u32,
    /// Maturity height (absolute).
    pub maturity_height: u32,
    /// CET bucket prefix bits (spec 02 §7 `m`).
    pub m: u8,
    /// Liquidation penalty, bps of face (spec 06 §8).
    pub penalty_bps: u32,
    /// Identifier of the oracle event series the checkpoints attest to.
    pub oracle_event_series: [u8; 32],
}

impl VaultTerms {
    /// Canonical big-endian field encoding (declaration order, spec 00
    /// §3): fixed-width integers, no length prefixes (the layout is
    /// fixed). Feeds `vault_id`.
    pub fn encode(&self) -> Vec<u8> {
        let mut b = Vec::with_capacity(8 + 8 + 4 + 4 + 4 + 4 + 1 + 4 + 32);
        b.extend_from_slice(&self.collateral_sats.to_be_bytes());
        b.extend_from_slice(&self.mint_micro_usd.to_be_bytes());
        b.extend_from_slice(&self.opening_cr_bps.to_be_bytes());
        b.extend_from_slice(&self.liq_cr_bps.to_be_bytes());
        b.extend_from_slice(&self.checkpoint_interval.to_be_bytes());
        b.extend_from_slice(&self.maturity_height.to_be_bytes());
        b.push(self.m);
        b.extend_from_slice(&self.penalty_bps.to_be_bytes());
        b.extend_from_slice(&self.oracle_event_series);
        b
    }

    /// `vault_id = TaggedHash("SatUSD/vault-contract/v1", encode)`.
    pub fn vault_id(&self) -> [u8; 32] {
        tagged_hash("SatUSD/vault-contract/v1", &self.encode())
    }
}

/// The collateral ratio, in basis points, at a given BTC/USD price:
/// `CR = collateral_value / face`. Truncating (floors toward the
/// reserve's favor — a lower reported CR is the conservative read).
pub fn cr_bps(collateral_sats: u64, mint_micro_usd: u64, price_usd: u32) -> u32 {
    if mint_micro_usd == 0 {
        return u32::MAX;
    }
    // collateral_value_µUSD = collateral_sats * price * MICRO / SATS_PER_BTC
    // cr_bps = collateral_value_µUSD * 10000 / face_µUSD
    //        = collateral_sats * price * MICRO * 10000 / (SATS_PER_BTC * face_µUSD)
    let num = u128::from(collateral_sats) * u128::from(price_usd) * MICRO_PER_USD * 10_000;
    let den = SATS_PER_BTC * u128::from(mint_micro_usd);
    (num / den).min(u128::from(u32::MAX)) as u32
}

/// Does the collateral meet the minimum opening CR at the reference
/// price? (spec 06 §3 step 2.)
pub fn opening_ok(terms: &VaultTerms, price_usd: u32) -> bool {
    cr_bps(terms.collateral_sats, terms.mint_micro_usd, price_usd) >= terms.opening_cr_bps
}

/// The highest BTC/USD price at which the vault is still in crash
/// territory (own CR ≤ liq_CR). Prices at or below this are crash
/// buckets; above, the vault glides (no CET broadcastable). Floors
/// (conservative — never marks a healthy price as a crash).
pub fn crash_price_ceiling(terms: &VaultTerms) -> u32 {
    if terms.collateral_sats == 0 {
        return u32::MAX;
    }
    // own CR ≤ liq_CR ⟺ price ≤ liq_cr_bps * face_µUSD * SATS_PER_BTC
    //                            / (collateral_sats * MICRO * 10000)
    let num = u128::from(terms.liq_cr_bps) * u128::from(terms.mint_micro_usd) * SATS_PER_BTC;
    let den = u128::from(terms.collateral_sats) * MICRO_PER_USD * 10_000;
    if den == 0 {
        return u32::MAX;
    }
    (num / den).min(u128::from(u32::MAX)) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn terms() -> VaultTerms {
        VaultTerms {
            collateral_sats: 1_172, // ~150% CR for $0.5 @ $64k (see funding test below)
            mint_micro_usd: 500_000, // $0.50
            opening_cr_bps: 15_000,
            liq_cr_bps: 11_000,
            checkpoint_interval: 144,
            maturity_height: 900_000,
            m: 6,
            penalty_bps: 500,
            oracle_event_series: [7u8; 32],
        }
    }

    #[test]
    fn cr_and_opening() {
        let t = terms();
        // 1172 sats @ $64000 ≈ $0.75008 ⇒ CR ≈ 150.0% of $0.50.
        let cr = cr_bps(t.collateral_sats, t.mint_micro_usd, 64_000);
        assert!((14_990..=15_010).contains(&cr), "CR≈150% got {cr}");
        assert!(opening_ok(&t, 64_000));
        assert!(!opening_ok(&t, 60_000), "lower price ⇒ under opening CR");
    }

    #[test]
    fn crash_ceiling_is_below_open() {
        let t = terms();
        let p = crash_price_ceiling(&t);
        // liq 110% sits below the 150% open price (~$64k) — crash near ~$47k.
        assert!(p < 64_000 && p > 40_000, "crash ceiling {p}");
        // At the ceiling, own CR ≈ liq_CR.
        assert!(cr_bps(t.collateral_sats, t.mint_micro_usd, p) <= t.liq_cr_bps);
        assert!(cr_bps(t.collateral_sats, t.mint_micro_usd, p + 2_000) > t.liq_cr_bps);
    }

    #[test]
    fn vault_id_is_deterministic_and_binds_terms() {
        let t = terms();
        assert_eq!(t.vault_id(), t.vault_id());
        let mut t2 = t.clone();
        t2.liq_cr_bps += 1;
        assert_ne!(t.vault_id(), t2.vault_id(), "a term change ⇒ a new vault_id");
    }
}
