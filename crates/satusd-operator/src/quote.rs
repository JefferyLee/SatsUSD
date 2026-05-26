//! Operator quote engine (PRD §5.D2, §14.6). Pure pricing math: given a
//! redemption amount and the oracle price, how much BTC the operator pays the
//! user (net of the operator fee). Mirrors the gross formula the state node's
//! ReserveClaim verifier uses (`claim.rs`), so a quote the operator honours will
//! reconcile with the reimbursement the reserve later pays.

/// A redemption quote, all amounts in their protocol units.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Quote {
    /// SatUSD being redeemed, in cent-atoms (decimals = 2).
    pub amount_atoms: u64,
    /// Oracle BTC/USD price × 10^8.
    pub price_e8: u64,
    /// Gross BTC owed for the redeemed value, in sats.
    pub gross_sats: u64,
    /// Operator fee, in sats (`gross · fee_bps / 10_000`).
    pub fee_sats: u64,
    /// Net BTC the user receives via the HTLC, in sats (`gross − fee`).
    pub user_payout_sats: u64,
}

/// Why a quote could not be produced.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuoteError {
    ZeroPrice,
    FeeTooHigh,
    Overflow,
}

/// `gross_sats = amount_atoms · 10^14 / price_e8` (the §5.D12 gross formula), then
/// `fee = gross · fee_bps / 10_000`. All arithmetic widens to u128 (§ safety).
pub fn quote(amount_atoms: u64, price_e8: u64, fee_bps: u16) -> Result<Quote, QuoteError> {
    if price_e8 == 0 {
        return Err(QuoteError::ZeroPrice);
    }
    if fee_bps > 10_000 {
        return Err(QuoteError::FeeTooHigh);
    }
    let gross = (amount_atoms as u128)
        .checked_mul(100_000_000_000_000)
        .ok_or(QuoteError::Overflow)?
        / price_e8 as u128;
    let fee = gross * fee_bps as u128 / 10_000;
    let gross_sats = u64::try_from(gross).map_err(|_| QuoteError::Overflow)?;
    let fee_sats = fee as u64;
    Ok(Quote {
        amount_atoms,
        price_e8,
        gross_sats,
        fee_sats,
        user_payout_sats: gross_sats - fee_sats,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const PRICE_50K: u64 = 5_000_000_000_000; // $50,000 × 10^8

    #[test]
    fn quote_matches_gross_formula() {
        // $40 (4000 cent-atoms) at $50k/BTC = 0.0008 BTC = 80_000 sats gross.
        let q = quote(4_000, PRICE_50K, 50).unwrap();
        assert_eq!(q.gross_sats, 80_000);
        assert_eq!(q.fee_sats, 400); // 0.50%
        assert_eq!(q.user_payout_sats, 79_600);
    }

    #[test]
    fn zero_fee_pays_full_gross() {
        let q = quote(100_000_000, PRICE_50K, 0).unwrap();
        assert_eq!(q.fee_sats, 0);
        assert_eq!(q.user_payout_sats, q.gross_sats);
    }

    #[test]
    fn rejects_bad_inputs() {
        assert_eq!(quote(4_000, 0, 50), Err(QuoteError::ZeroPrice));
        assert_eq!(quote(4_000, PRICE_50K, 10_001), Err(QuoteError::FeeTooHigh));
    }
}
