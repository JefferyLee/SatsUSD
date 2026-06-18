//! Liveness escape via timelock encryption (spec 03 §3.5). Alongside each
//! announcement batch the oracle (or FROST cohort) publishes the batch's
//! nonce secrets encrypted to a future **drand round**. If the oracle goes
//! silent, the beacon's routine operation releases the round signature after
//! `delay_s`, decrypting the escape — so oracle silence is a bounded delay,
//! never a fund loss (the rail's lock template routes the "all outcomes
//! released" condition to the refund-equivalent CET; spec 02 §3.4).
//!
//! This module provides the **structure** and the encrypt-then-release
//! **flow**: the [`TlockEscape`] container, the drand round arithmetic, and a
//! [`Timelock`] trait abstracting the timelock-encryption primitive (the real
//! implementation is drand-IBE over BLS12-381 — a heavy pairing dependency,
//! kept behind this trait so the protocol is testable without it). Two pieces
//! remain a **pending proposal** (spec 03 §6 open item 1): the exact on-chain
//! refund-CET wiring, and — for the threshold cohort — which secret the
//! quorum seals so that release makes every outcome reconstructible.

/// drand `quicknet` chain (the unchained, 3-second, timelock-capable beacon).
pub const QUICKNET_CHAIN: [u8; 32] = [
    0x52, 0xdb, 0x9b, 0xa7, 0x0e, 0x0c, 0xc0, 0xf6, 0xea, 0xf7, 0x80, 0x3d, 0xd0, 0x74, 0x47, 0xa1,
    0xf5, 0x47, 0x77, 0x35, 0xfd, 0x3f, 0x66, 0x17, 0x92, 0xba, 0x94, 0x60, 0x0c, 0x84, 0xe9, 0x71,
];
pub const QUICKNET_GENESIS: u64 = 1_692_803_367;
pub const QUICKNET_PERIOD_S: u64 = 3;
/// RECOMMENDED grace before the beacon releases the secrets (spec 03 §3.5).
pub const DEFAULT_DELAY_S: u64 = 600;

/// The first drand round whose beacon is published at or after `unix_time`.
pub fn round_at(genesis: u64, period_s: u64, unix_time: u64) -> u64 {
    if period_s == 0 || unix_time <= genesis {
        return 1;
    }
    // Round r is emitted at genesis + (r-1)·period; the first round at-or-after
    // `unix_time` is ceil((unix_time − genesis)/period) + 1.
    let elapsed = unix_time - genesis;
    elapsed.div_ceil(period_s) + 1
}

/// The quicknet round whose release frees a batch maturing at
/// `max_maturity_ts`, after the grace `delay_s` (spec 03 §3.5).
pub fn quicknet_escape_round(max_maturity_ts: u64, delay_s: u64) -> u64 {
    round_at(QUICKNET_GENESIS, QUICKNET_PERIOD_S, max_maturity_ts + delay_s)
}

/// Timelock encryption to a drand round: `encrypt` uses only the round's
/// public identity (available now); `decrypt` needs the round's `beacon`
/// (the released BLS signature, available only at round time). The real
/// implementation is drand-IBE (tlock); this trait keeps the BLS pairing
/// dependency out of the protocol crate.
pub trait Timelock {
    fn encrypt(&self, chain: &[u8; 32], round: u64, plaintext: &[u8]) -> Vec<u8>;
    /// Returns the plaintext iff `beacon` is the round's released signature.
    fn decrypt(&self, chain: &[u8; 32], round: u64, ciphertext: &[u8], beacon: Option<&[u8]>) -> Option<Vec<u8>>;
}

/// The published escape: the batch's nonce secrets sealed to a drand round.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TlockEscape {
    pub drand_chain: [u8; 32],
    pub round: u64,
    pub ciphertext: Vec<u8>,
}

impl TlockEscape {
    /// Seal `batch_secrets` (e.g. the concatenated per-digit nonce secrets of
    /// an announcement batch) to `round` on `chain`.
    pub fn seal<T: Timelock>(tl: &T, chain: [u8; 32], round: u64, batch_secrets: &[u8]) -> Self {
        TlockEscape { drand_chain: chain, round, ciphertext: tl.encrypt(&chain, round, batch_secrets) }
    }

    /// Open the escape once the beacon for `round` is available; `None` while
    /// the beacon is still in the future (the oracle's silence is, until
    /// then, just a delay).
    pub fn open<T: Timelock>(&self, tl: &T, beacon: Option<&[u8]>) -> Option<Vec<u8>> {
        tl.decrypt(&self.drand_chain, self.round, &self.ciphertext, beacon)
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut b = self.drand_chain.to_vec();
        b.extend_from_slice(&self.round.to_be_bytes());
        b.extend_from_slice(&(self.ciphertext.len() as u32).to_be_bytes());
        b.extend_from_slice(&self.ciphertext);
        b
    }

    pub fn from_bytes(b: &[u8]) -> Option<Self> {
        if b.len() < 44 {
            return None;
        }
        let drand_chain: [u8; 32] = b[..32].try_into().ok()?;
        let round = u64::from_be_bytes(b[32..40].try_into().ok()?);
        let len = u32::from_be_bytes(b[40..44].try_into().ok()?) as usize;
        let ciphertext = b.get(44..44 + len)?.to_vec();
        Some(TlockEscape { drand_chain, round, ciphertext })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use satusd_rail::encode::tagged_hash;

    /// A test stub for the timelock primitive: it models the *gating*
    /// contract (decrypt requires the round's beacon) without the real
    /// IBE/pairing — the keystream is a tagged hash, and decrypt refuses
    /// unless the caller presents the matching beacon for the round.
    struct MockTimelock;

    fn mock_beacon(chain: &[u8; 32], round: u64) -> Vec<u8> {
        let mut b = chain.to_vec();
        b.extend_from_slice(&round.to_be_bytes());
        tagged_hash("test/drand-beacon/v0", &b).to_vec()
    }
    fn keystream(chain: &[u8; 32], round: u64, len: usize) -> Vec<u8> {
        let mut out = Vec::with_capacity(len);
        let mut ctr = 0u32;
        while out.len() < len {
            let mut b = chain.to_vec();
            b.extend_from_slice(&round.to_be_bytes());
            b.extend_from_slice(&ctr.to_be_bytes());
            out.extend_from_slice(&tagged_hash("test/tlock-keystream/v0", &b));
            ctr += 1;
        }
        out.truncate(len);
        out
    }
    impl Timelock for MockTimelock {
        fn encrypt(&self, chain: &[u8; 32], round: u64, pt: &[u8]) -> Vec<u8> {
            let ks = keystream(chain, round, pt.len());
            pt.iter().zip(ks).map(|(p, k)| p ^ k).collect()
        }
        fn decrypt(&self, chain: &[u8; 32], round: u64, ct: &[u8], beacon: Option<&[u8]>) -> Option<Vec<u8>> {
            if beacon? != mock_beacon(chain, round).as_slice() {
                return None;
            }
            let ks = keystream(chain, round, ct.len());
            Some(ct.iter().zip(ks).map(|(c, k)| c ^ k).collect())
        }
    }

    #[test]
    fn round_arithmetic() {
        assert_eq!(round_at(1000, 3, 999), 1, "before genesis → round 1");
        assert_eq!(round_at(1000, 3, 1000), 1);
        assert_eq!(round_at(1000, 3, 1003), 2);
        assert_eq!(round_at(1000, 3, 1004), 3);
        // A maturity + 600s grace maps to a concrete future quicknet round.
        let r = quicknet_escape_round(QUICKNET_GENESIS + 30, DEFAULT_DELAY_S);
        assert_eq!(r, round_at(QUICKNET_GENESIS, QUICKNET_PERIOD_S, QUICKNET_GENESIS + 630));
    }

    #[test]
    fn silence_is_a_bounded_delay_not_a_loss() {
        // Seal a batch's nonce secrets; before the beacon they stay sealed,
        // after release they come back verbatim — the escape fires only on
        // (and bounded by) the beacon's routine operation.
        let tl = MockTimelock;
        let secrets = [7u8; 64]; // stand-in for concat(k_i) of a batch
        let round = quicknet_escape_round(QUICKNET_GENESIS + 1_000, DEFAULT_DELAY_S);
        let escape = TlockEscape::seal(&tl, QUICKNET_CHAIN, round, &secrets);

        // Oracle still live / beacon not yet out → cannot open.
        assert_eq!(escape.open(&tl, None), None);
        let wrong = [0u8; 32];
        assert_eq!(escape.open(&tl, Some(&wrong)), None);

        // Beacon released → the secrets are recovered exactly.
        let beacon = mock_beacon(&QUICKNET_CHAIN, round);
        assert_eq!(escape.open(&tl, Some(&beacon)).as_deref(), Some(&secrets[..]));
    }

    #[test]
    fn escape_bytes_round_trip() {
        let tl = MockTimelock;
        let escape = TlockEscape::seal(&tl, QUICKNET_CHAIN, 42, &[1, 2, 3, 4, 5]);
        assert_eq!(TlockEscape::from_bytes(&escape.to_bytes()).unwrap(), escape);
        assert!(TlockEscape::from_bytes(&[0u8; 10]).is_none());
    }
}
