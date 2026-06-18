//! FROST threshold attestation (spec 03 §5.7): a *t*-of-*n* cohort that
//! presents the decentralised stake-weighted median (§5.5, [`crate::median`])
//! to a DLC as **one** BIP-340 key — escaping the `C(n,k)` multi-oracle
//! adaptor blow-up (§3.4). The cohort runs **Pedersen distributed key
//! generation** (no trusted dealer) once, yielding a group key `PK` and one
//! secret share per participant; each event a quorum threshold-signs the
//! median's digit decomposition into the *same* [`Announcement`] /
//! [`Attestation`] a single [`crate::oracle::Oracle`] emits — so the bucket
//! consumer (`satusd-rail1::cet`) is unchanged: it just uses `PK` as the
//! oracle key.
//!
//! Correctness rests on two identities. Lagrange interpolation over the
//! quorum reconstructs the (never-materialised) group secret in the
//! exponent: `Σ_{j∈Q} λ_j·x_j = s` with `PK = s·G`. And the per-digit
//! aggregate `(R_i, s_i)` is a textbook BIP-340 signature under `PK`:
//! `s_i·G = R_i + e_i·PK`, so `anticipation_point(R_i, PK, msg_i) = s_i·G`
//! — exactly the consumer contract the single oracle satisfies.
//!
//! v0 scope: the DKG ceremony (commitment proofs-of-knowledge + dealt-share
//! verification) and the threshold signing math, simulated in one process —
//! the wire transport between participants is the daemon's. Per-(event,digit)
//! nonces are derived deterministically from each participant's seed; the
//! FROST binding-factor two-nonce hardening (RFC 9591) against concurrent /
//! adaptive sessions is a named follow-up (the attested message set here is
//! the fixed digit decomposition, not adversary-chosen). The dlcspecs
//! announcement *signature* (vs the attestation signatures the DLC consumes)
//! is likewise deferred.

use satusd_rail::encode::tagged_hash;
use secp256k1::{Parity, PublicKey, Scalar, Secp256k1, SecretKey, XOnlyPublicKey};

use crate::event::{attestation_msg, digits, event_id, BASE, NB_DIGITS};
use crate::oracle::{Announcement, Attestation};
use crate::schnorr::{challenge, invert_mod_n, sub_mod_n};
use crate::tlv;

#[derive(Debug, PartialEq, Eq)]
pub enum FrostError {
    /// A dealt share did not match the dealer's broadcast commitment.
    BadShare,
    /// A participant's commitment proof-of-knowledge did not verify.
    BadPok,
    /// The quorum is smaller than the threshold `t`.
    QuorumTooSmall,
    /// A quorum index is out of range or repeated.
    BadQuorum,
    /// Threshold/participant counts are degenerate.
    BadParams,
    /// Underlying secp256k1 failure.
    Crypto,
}

impl From<secp256k1::Error> for FrostError {
    fn from(_: secp256k1::Error) -> Self {
        FrostError::Crypto
    }
}

const COEFF_TAG: &str = "SatUSD/frost/dkg-coeff/v1";
const POK_TAG: &str = "SatUSD/frost/dkg-pok/v1";
const POK_NONCE_TAG: &str = "SatUSD/frost/dkg-pok-nonce/v1";
const PARTICIPANT_SEED_TAG: &str = "SatUSD/frost/participant-seed/v1";
const HIDING_NONCE_TAG: &str = "SatUSD/frost/hiding-nonce/v1";
const BINDING_NONCE_TAG: &str = "SatUSD/frost/binding-nonce/v1";
const BINDING_FACTOR_TAG: &str = "SatUSD/frost/binding-factor/v1";

fn scalar(bytes: [u8; 32]) -> SecretKey {
    SecretKey::from_byte_array(bytes).expect("hash output is a scalar in range")
}

/// A nonzero scalar from a small participant index `j` (1-based).
fn index_scalar(j: u16) -> SecretKey {
    let mut b = [0u8; 32];
    b[30..].copy_from_slice(&j.to_be_bytes());
    SecretKey::from_byte_array(b).expect("index 1..=n is a nonzero scalar")
}

/// Horner evaluation of the polynomial `coeffs` (`a0 + a1·x + …`) at `x`.
fn poly_eval(coeffs: &[SecretKey], x: &SecretKey) -> Result<SecretKey, secp256k1::Error> {
    let mut acc = coeffs[coeffs.len() - 1];
    for c in coeffs.iter().rev().skip(1) {
        acc = acc.mul_tweak(&Scalar::from(*x))?.add_tweak(&Scalar::from(*c))?;
    }
    Ok(acc)
}

/// `Σ_k x^k · C_k` — the commitment evaluated at `x` (the public check for
/// a dealt share: `share·G` must equal this).
fn eval_commitment(commitment: &[PublicKey], x: &SecretKey) -> Result<PublicKey, secp256k1::Error> {
    let secp = Secp256k1::new();
    let mut acc: Option<PublicKey> = None;
    let mut xpow: Option<SecretKey> = None; // x^k; None encodes x^0 = 1
    for ck in commitment {
        let term = match &xpow {
            None => *ck,
            Some(p) => ck.mul_tweak(&secp, &Scalar::from(*p))?,
        };
        acc = Some(match acc {
            None => term,
            Some(a) => a.combine(&term)?,
        });
        xpow = Some(match xpow {
            None => *x,
            Some(p) => p.mul_tweak(&Scalar::from(*x))?,
        });
    }
    acc.ok_or(secp256k1::Error::InvalidPublicKeySum)
}

/// The Lagrange coefficient `λ_j = Π_{m∈Q, m≠j} x_m/(x_m − x_j)` at 0 —
/// the weight that makes `Σ_{j∈Q} λ_j·f(j) = f(0)`.
fn lagrange_at_zero(j: u16, quorum: &[u16]) -> Result<SecretKey, FrostError> {
    let xj = index_scalar(j);
    let mut num: Option<SecretKey> = None;
    let mut den: Option<SecretKey> = None;
    for &m in quorum {
        if m == j {
            continue;
        }
        let xm = index_scalar(m);
        num = Some(match num {
            None => xm,
            Some(p) => p.mul_tweak(&Scalar::from(xm))?,
        });
        let diff = sub_mod_n(&xm, &xj)?;
        den = Some(match den {
            None => diff,
            Some(p) => p.mul_tweak(&Scalar::from(diff))?,
        });
    }
    let num = num.ok_or(FrostError::QuorumTooSmall)?;
    let den = den.ok_or(FrostError::QuorumTooSmall)?;
    Ok(num.mul_tweak(&Scalar::from(invert_mod_n(&den)?))?)
}

/// A participant's per-(event, digit) FROST nonce pair: the hiding nonce
/// `d_j` and the binding nonce `e_j`. The effective nonce a signer
/// contributes is `d_j + ρ_j·e_j`, where the binding factor `ρ_j` ties it
/// to the full commitment set (below) — so no participant can steer the
/// aggregate nonce by choosing their commitment after seeing the others'
/// (the Drijvers / rogue-nonce defence, RFC 9591 §4.4).
fn nonce_pair(seed: &[u8; 32], event_id: &str, digit: u16) -> (SecretKey, SecretKey) {
    let mut b = seed.to_vec();
    b.extend_from_slice(event_id.as_bytes());
    b.extend_from_slice(&digit.to_be_bytes());
    (scalar(tagged_hash(HIDING_NONCE_TAG, &b)), scalar(tagged_hash(BINDING_NONCE_TAG, &b)))
}

/// The per-signer binding factor `ρ_j = H(j ‖ event ‖ digit ‖ B)`, where
/// `B` is the ordered commitment set `{(i, D_i, E_i)}` of the quorum. It
/// binds each signer's nonce to *everyone's* commitments, so the aggregate
/// nonce is a non-linear function of all of them. The attested message (the
/// digit value) is deliberately NOT an input: in the DLC announce-then-
/// attest model the nonce is committed before the outcome and reused for
/// whichever digit realises — signing two values for one digit is
/// equivocation, which leaks the key by design (EOTS, spec 03 §3.3), not a
/// forgery to defend against.
fn binding_factor(j: u16, b_encoded: &[u8]) -> SecretKey {
    let mut rb = j.to_be_bytes().to_vec();
    rb.extend_from_slice(b_encoded);
    scalar(tagged_hash(BINDING_FACTOR_TAG, &rb))
}

/// One participant's DKG polynomial + the public artifacts it broadcasts.
struct Dealer {
    index: u16,
    seed: [u8; 32],
    poly: Vec<SecretKey>,
    commitment: Vec<PublicKey>,
    pok: [u8; 64],
}

fn pok_message(index: u16, c0: &PublicKey) -> [u8; 32] {
    let mut b = index.to_be_bytes().to_vec();
    b.extend_from_slice(&c0.serialize());
    tagged_hash(POK_TAG, &b)
}

impl Dealer {
    fn new(index: u16, t: u16, master_seed: &[u8; 32]) -> Result<Self, secp256k1::Error> {
        let secp = Secp256k1::new();
        let mut sb = master_seed.to_vec();
        sb.extend_from_slice(&index.to_be_bytes());
        let seed = tagged_hash(PARTICIPANT_SEED_TAG, &sb);

        let poly: Vec<SecretKey> = (0..t)
            .map(|k| {
                let mut cb = seed.to_vec();
                cb.extend_from_slice(&k.to_be_bytes());
                scalar(tagged_hash(COEFF_TAG, &cb))
            })
            .collect();
        let commitment: Vec<PublicKey> = poly.iter().map(|a| a.public_key(&secp)).collect();

        // Proof of knowledge of a0 = dlog(C0): a BIP-340 signature under a0.
        let pok = crate::schnorr::sign_with_nonce(
            &poly[0].secret_bytes(),
            &tagged_hash(POK_NONCE_TAG, &seed),
            &pok_message(index, &commitment[0]),
        )?;
        Ok(Dealer { index, seed, poly, commitment, pok })
    }
}

fn verify_pok(index: u16, commitment: &[PublicKey], pok: &[u8; 64]) -> bool {
    let secp = Secp256k1::new();
    let (c0x, _) = commitment[0].x_only_public_key();
    secp.verify_schnorr(
        &secp256k1::schnorr::Signature::from_byte_array(*pok),
        &pok_message(index, &commitment[0]),
        &c0x,
    )
    .is_ok()
}

/// A *t*-of-*n* FROST cohort after a completed Pedersen DKG.
pub struct Cohort {
    pub n: u16,
    pub t: u16,
    /// `PK`'s BIP-340 x-only key — the oracle key the DLC consumes.
    pub group_pubkey: [u8; 32],
    pk_odd: bool,
    seeds: Vec<[u8; 32]>,   // participant nonce seeds, index j stored at j-1
    shares: Vec<SecretKey>, // long-term secret shares x_j, index j at j-1
}

impl Cohort {
    /// Run the full Pedersen DKG (no trusted dealer): every participant
    /// commits to a polynomial with a proof-of-knowledge of its constant
    /// term, deals shares to the others, and each share is checked against
    /// the dealer's broadcast commitment. The group key is the sum of the
    /// constant-term commitments; nobody ever holds the group secret.
    ///
    /// Simulated in one process — `master_seed` derives each participant's
    /// polynomial and nonce seed deterministically (real participants sample
    /// locally and exchange over the wire).
    pub fn keygen(n: u16, t: u16, master_seed: &[u8; 32]) -> Result<Self, FrostError> {
        if t == 0 || n == 0 || t > n {
            return Err(FrostError::BadParams);
        }
        let secp = Secp256k1::new();

        // Round 1: each participant broadcasts a commitment + PoK.
        let dealers: Vec<Dealer> = (1..=n)
            .map(|i| Dealer::new(i, t, master_seed))
            .collect::<Result<_, _>>()?;
        for d in &dealers {
            if !verify_pok(d.index, &d.commitment, &d.pok) {
                return Err(FrostError::BadPok);
            }
        }

        // Round 2: each participant j collects + verifies a dealt share from
        // every dealer i, then sums them into its long-term share x_j.
        let mut shares = Vec::with_capacity(n as usize);
        for j in 1..=n {
            let xj_scalar = index_scalar(j);
            let mut x_j: Option<SecretKey> = None;
            for d in &dealers {
                let share = poly_eval(&d.poly, &xj_scalar)?;
                if share.public_key(&secp) != eval_commitment(&d.commitment, &xj_scalar)? {
                    return Err(FrostError::BadShare);
                }
                x_j = Some(match x_j {
                    None => share,
                    Some(acc) => acc.add_tweak(&Scalar::from(share))?,
                });
            }
            shares.push(x_j.ok_or(FrostError::BadParams)?);
        }

        // The group key PK = Σ_i C_{i,0}.
        let mut pk: Option<PublicKey> = None;
        for d in &dealers {
            pk = Some(match pk {
                None => d.commitment[0],
                Some(acc) => acc.combine(&d.commitment[0])?,
            });
        }
        let (px, parity) = pk.ok_or(FrostError::BadParams)?.x_only_public_key();

        Ok(Cohort {
            n,
            t,
            group_pubkey: px.serialize(),
            pk_odd: parity == Parity::Odd,
            seeds: dealers.iter().map(|d| d.seed).collect(),
            shares,
        })
    }

    fn check_quorum(&self, quorum: &[u16]) -> Result<(), FrostError> {
        if quorum.len() < self.t as usize {
            return Err(FrostError::QuorumTooSmall);
        }
        for (i, &j) in quorum.iter().enumerate() {
            if j == 0 || j > self.n || quorum[..i].contains(&j) {
                return Err(FrostError::BadQuorum);
            }
        }
        Ok(())
    }

    /// A FROST signing session for one digit: the binding-factor aggregate
    /// nonce `R = Σ_{j∈Q} (D_j + ρ_j·E_j)` and, in quorum order, each
    /// signer's effective nonce scalar `d_j + ρ_j·e_j` (so `Σ eff_j·G = R`).
    /// Both announce (which commits `R`) and attest (which consumes the
    /// effective nonces) derive this identically.
    fn nonce_session(
        &self,
        quorum: &[u16],
        event_id: &str,
        digit: u16,
    ) -> Result<(PublicKey, Vec<SecretKey>), secp256k1::Error> {
        let secp = Secp256k1::new();
        // Gather (j, d_j, e_j, D_j, E_j) and encode the commitment set B.
        let mut parts = Vec::with_capacity(quorum.len());
        let mut b_encoded = event_id.as_bytes().to_vec();
        b_encoded.extend_from_slice(&digit.to_be_bytes());
        for &j in quorum {
            let (d, e) = nonce_pair(&self.seeds[(j - 1) as usize], event_id, digit);
            let (dpt, ept) = (d.public_key(&secp), e.public_key(&secp));
            b_encoded.extend_from_slice(&j.to_be_bytes());
            b_encoded.extend_from_slice(&dpt.serialize());
            b_encoded.extend_from_slice(&ept.serialize());
            parts.push((j, d, e, dpt, ept));
        }
        // Per-signer binding factor → aggregate nonce + effective nonces.
        let mut r: Option<PublicKey> = None;
        let mut effs = Vec::with_capacity(quorum.len());
        for (j, d, e, dpt, ept) in &parts {
            let rho = binding_factor(*j, &b_encoded);
            // R contribution: D_j + ρ_j·E_j.
            let term = dpt.combine(&ept.mul_tweak(&secp, &Scalar::from(rho))?)?;
            r = Some(match r {
                None => term,
                Some(acc) => acc.combine(&term)?,
            });
            // Effective nonce: d_j + ρ_j·e_j.
            effs.push(d.add_tweak(&Scalar::from(e.mul_tweak(&Scalar::from(rho))?))?);
        }
        Ok((r.expect("non-empty quorum"), effs))
    }

    /// Announce the per-digit aggregate nonce points for the tick at
    /// `unix_ts`, signed by `quorum`. (The dlcspecs announcement *signature*
    /// is deferred — see the module note; the bucket consumer uses only the
    /// nonce points + `group_pubkey`.)
    pub fn announce(&self, unix_ts: u64, quorum: &[u16]) -> Result<Announcement, FrostError> {
        self.check_quorum(quorum)?;
        let id = event_id(unix_ts);
        let mut points = Vec::with_capacity(NB_DIGITS as usize);
        for i in 0..NB_DIGITS {
            let (r, _) = self.nonce_session(quorum, &id, i)?;
            points.push(r.x_only_public_key().0.serialize());
        }
        let descriptor = tlv::digit_descriptor(BASE, false, "USD/BTC", 0, NB_DIGITS);
        let event = tlv::oracle_event(&points, unix_ts as u32, &descriptor, &id);
        Ok(Announcement { event_id: id, nonce_points: points, tlv_bytes: event })
    }

    /// Threshold-attest `price_usd` for the tick at `unix_ts`: each digit's
    /// aggregate `(R_i, s_i)` is a BIP-340 signature under `group_pubkey`,
    /// byte-identical in structure to a single oracle's attestation.
    pub fn attest(&self, unix_ts: u64, price_usd: u32, quorum: &[u16]) -> Result<Attestation, FrostError> {
        self.check_quorum(quorum)?;
        let id = event_id(unix_ts);
        let px = XOnlyPublicKey::from_byte_array(self.group_pubkey)?;

        // Lagrange weights are fixed by the quorum (not the digit).
        let lambdas: Vec<SecretKey> =
            quorum.iter().map(|&j| lagrange_at_zero(j, quorum)).collect::<Result<_, _>>()?;

        let ds = digits(price_usd);
        let mut sigs = Vec::with_capacity(ds.len());
        for (i, d) in ds.iter().enumerate() {
            let digit = i as u16;
            let (r, effs) = self.nonce_session(quorum, &id, digit)?;
            let (rx, r_parity) = r.x_only_public_key();
            let r_odd = r_parity == Parity::Odd;
            let e = challenge(&rx, &px, &attestation_msg(*d));

            let mut s: Option<SecretKey> = None;
            for (idx, &j) in quorum.iter().enumerate() {
                // The binding-factor effective nonce, negated if R is odd-Y.
                let kj = if r_odd { effs[idx].negate() } else { effs[idx] };
                // e·λ_j·x_j, negated when PK is odd-Y (signing under −s).
                let key_part = lambdas[idx]
                    .mul_tweak(&Scalar::from(self.shares[(j - 1) as usize]))?
                    .mul_tweak(&e)?;
                let key_part = if self.pk_odd { key_part.negate() } else { key_part };
                let zj = kj.add_tweak(&Scalar::from(key_part))?;
                s = Some(match s {
                    None => zj,
                    Some(acc) => acc.add_tweak(&Scalar::from(zj))?,
                });
            }
            let mut sig = [0u8; 64];
            sig[..32].copy_from_slice(&rx.serialize());
            sig[32..].copy_from_slice(&s.expect("non-empty quorum").secret_bytes());
            sigs.push(sig);
        }

        let outcomes: Vec<&str> = ds.iter().map(|d| if *d == 0 { "0" } else { "1" }).collect();
        let tlv_bytes = tlv::oracle_attestation(&id, &self.group_pubkey, &sigs, &outcomes);
        Ok(Attestation { event_id: id, price_usd, signatures: sigs, tlv_bytes })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schnorr::anticipation_point;
    use secp256k1::schnorr::Signature;

    #[test]
    fn dkg_then_threshold_attestation_verifies_under_one_group_key() {
        // 3-of-5 cohort; any threshold quorum signs under the SAME group key.
        let cohort = Cohort::keygen(5, 3, &[7u8; 32]).unwrap();
        let ts = 1_700_000_000u64;
        let price = 60_123u32;
        let quorum = [1u16, 3, 5];

        let ann = cohort.announce(ts, &quorum).unwrap();
        let att = cohort.attest(ts, price, &quorum).unwrap();

        let secp = Secp256k1::new();
        let px = XOnlyPublicKey::from_byte_array(cohort.group_pubkey).unwrap();
        let ds = digits(price);
        for (i, sig) in att.signatures.iter().enumerate() {
            // R matches the announced aggregate nonce.
            assert_eq!(sig[..32], ann.nonce_points[i], "digit {i} aggregate nonce");
            // The aggregate is a valid BIP-340 signature under the group key,
            // checked by an INDEPENDENT verifier.
            secp.verify_schnorr(&Signature::from_byte_array(*sig), &attestation_msg(ds[i]), &px)
                .unwrap_or_else(|e| panic!("digit {i} threshold sig must verify: {e}"));
        }
    }

    #[test]
    fn consumer_contract_holds_anticipation_point_equals_secret_point() {
        // The DLC bucket contract: for each digit, the point computable from
        // the ANNOUNCEMENT alone equals s·G of the ATTESTATION's scalar —
        // identical to a single oracle, so rail1::cet is unchanged.
        let cohort = Cohort::keygen(4, 2, &[9u8; 32]).unwrap();
        let ts = 1_700_000_001u64;
        let price = 99_999u32;
        let quorum = [2u16, 4];

        let ann = cohort.announce(ts, &quorum).unwrap();
        let att = cohort.attest(ts, price, &quorum).unwrap();

        let secp = Secp256k1::new();
        let px = XOnlyPublicKey::from_byte_array(cohort.group_pubkey).unwrap();
        let ds = digits(price);
        for (i, d) in ds.iter().enumerate() {
            let rx = XOnlyPublicKey::from_byte_array(ann.nonce_points[i]).unwrap();
            let point = anticipation_point(&rx, &px, &attestation_msg(*d)).unwrap();
            let s = SecretKey::from_byte_array(att.signatures[i][32..].try_into().unwrap()).unwrap();
            assert_eq!(point, s.public_key(&secp), "digit {i}: anticipation point ≠ s·G");
        }
    }

    #[test]
    fn different_quorums_yield_the_same_group_key_attestation() {
        // Any t-subset produces a valid attestation under the one group key.
        let cohort = Cohort::keygen(5, 3, &[11u8; 32]).unwrap();
        let ts = 1_700_000_002u64;
        let price = 42_000u32;
        let secp = Secp256k1::new();
        let px = XOnlyPublicKey::from_byte_array(cohort.group_pubkey).unwrap();
        let ds = digits(price);

        for quorum in [[1u16, 2, 3], [2, 4, 5], [1, 3, 5]] {
            let att = cohort.attest(ts, price, &quorum).unwrap();
            for (i, sig) in att.signatures.iter().enumerate() {
                secp.verify_schnorr(&Signature::from_byte_array(*sig), &attestation_msg(ds[i]), &px)
                    .unwrap_or_else(|_| panic!("quorum {quorum:?} digit {i} must verify"));
            }
        }
    }

    #[test]
    fn quorum_below_threshold_is_refused() {
        let cohort = Cohort::keygen(5, 3, &[13u8; 32]).unwrap();
        assert!(matches!(cohort.attest(1_700_000_003, 50_000, &[1, 2]), Err(FrostError::QuorumTooSmall)));
        assert!(matches!(cohort.announce(1_700_000_003, &[1, 2]), Err(FrostError::QuorumTooSmall)));
    }

    #[test]
    fn dkg_rejects_a_tampered_share() {
        // The DKG's share-vs-commitment check is what makes it dealer-free:
        // a dealt share that doesn't match the broadcast commitment is caught.
        let secp = Secp256k1::new();
        let d = Dealer::new(1, 3, &[5u8; 32]).unwrap();
        let xj = index_scalar(4);
        let good = poly_eval(&d.poly, &xj).unwrap();
        assert_eq!(good.public_key(&secp), eval_commitment(&d.commitment, &xj).unwrap());
        // A wrong share (off by one) fails the commitment check.
        let bad = good.add_tweak(&Scalar::from(index_scalar(1))).unwrap();
        assert_ne!(bad.public_key(&secp), eval_commitment(&d.commitment, &xj).unwrap());
    }

    #[test]
    fn dkg_rejects_a_forged_pok() {
        let d = Dealer::new(2, 2, &[6u8; 32]).unwrap();
        assert!(verify_pok(d.index, &d.commitment, &d.pok));
        // A PoK bound to the wrong index does not verify.
        assert!(!verify_pok(99, &d.commitment, &d.pok));
        let mut forged = d.pok;
        forged[10] ^= 1;
        assert!(!verify_pok(d.index, &d.commitment, &forged));
    }
}
