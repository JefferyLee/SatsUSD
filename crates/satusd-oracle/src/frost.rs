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
//! This module is the **distributed protocol**: each participant holds only
//! its own secret and exchanges explicit, independently-verifiable messages
//! ([`Round1Package`], [`Round2Share`] for DKG; [`NonceCommitment`],
//! [`SigShare`] for signing), combined by a pure [`aggregate_attestation`].
//! [`Cohort`] is a single-process simulator over these primitives (used by
//! tests and `satusd-rail1`); the `cohortd` daemon drives the same
//! primitives over a directory transport.
//!
//! Correctness rests on two identities. Lagrange interpolation over the
//! quorum reconstructs the (never-materialised) group secret in the
//! exponent: `Σ_{j∈Q} λ_j·x_j = s` with `PK = s·G`. And the per-digit
//! aggregate `(R_i, s_i)` is a textbook BIP-340 signature under `PK`:
//! `s_i·G = R_i + e_i·PK`, so `anticipation_point(R_i, PK, msg_i) = s_i·G`
//! — exactly the consumer contract the single oracle satisfies.
//!
//! v0 scope: per-(event,digit) signing nonces are derived deterministically
//! from each participant's seed (restart-safe, like `oracled`'s file state);
//! nonce generation is binding-factor hardened (RFC 9591 two-nonce, below).
//! The dlcspecs announcement *signature* (vs the attestation signatures the
//! DLC consumes) and the §3.5 tlock escape ([`crate::tlock`]) compose on top.

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
    /// A required message (round-1 package, dealt share, commitment, or
    /// signature share) for a participant was missing.
    MissingMessage,
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

// ===================== leaf scalar / point math =====================

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
/// to the full commitment set — so no participant can steer the aggregate
/// nonce by choosing its commitment after seeing the others' (the Drijvers
/// / rogue-nonce defence, RFC 9591 §4.4).
fn nonce_pair(seed: &[u8; 32], event_id: &str, digit: u16) -> (SecretKey, SecretKey) {
    let mut b = seed.to_vec();
    b.extend_from_slice(event_id.as_bytes());
    b.extend_from_slice(&digit.to_be_bytes());
    (scalar(tagged_hash(HIDING_NONCE_TAG, &b)), scalar(tagged_hash(BINDING_NONCE_TAG, &b)))
}

/// The per-signer binding factor `ρ_j = H(j ‖ B)`, where `B` encodes the
/// event, digit, and the ordered commitment set `{(i, D_i, E_i)}` of the
/// quorum. It binds each signer's nonce to *everyone's* commitments, so the
/// aggregate nonce is a non-linear function of all of them. The attested
/// message (the digit value) is deliberately NOT an input: in the DLC
/// announce-then-attest model the nonce is committed before the outcome and
/// reused for whichever digit realises — signing two values for one digit is
/// equivocation, which leaks the key by design (EOTS, spec 03 §3.3), not a
/// forgery to defend against.
fn binding_factor(j: u16, b_encoded: &[u8]) -> SecretKey {
    let mut rb = j.to_be_bytes().to_vec();
    rb.extend_from_slice(b_encoded);
    scalar(tagged_hash(BINDING_FACTOR_TAG, &rb))
}

/// One signer's nonce commitment for a digit: `(D_j, E_j) = (d_j·G, e_j·G)`.
type DigitCommit = (PublicKey, PublicKey);

/// Encode the commitment set `B` for a (event, digit) over the quorum, in
/// quorum order — the binding-factor input shared by announce, sign, and
/// aggregate so all three derive the identical session.
fn encode_commitment_set(event_id: &str, digit: u16, quorum: &[u16], des: &[DigitCommit]) -> Vec<u8> {
    let mut b = event_id.as_bytes().to_vec();
    b.extend_from_slice(&digit.to_be_bytes());
    for (j, (d, e)) in quorum.iter().zip(des) {
        b.extend_from_slice(&j.to_be_bytes());
        b.extend_from_slice(&d.serialize());
        b.extend_from_slice(&e.serialize());
    }
    b
}

/// A FROST signing session for one digit, derived purely from the public
/// commitments: the binding-factor aggregate nonce `R = Σ (D_j + ρ_j·E_j)`
/// and the per-signer binding factors `ρ_j` (quorum order). Announce, sign,
/// and aggregate all call this, so they agree on `R` and `ρ`.
fn nonce_session(
    event_id: &str,
    digit: u16,
    quorum: &[u16],
    des: &[DigitCommit],
) -> Result<(PublicKey, Vec<SecretKey>), secp256k1::Error> {
    let secp = Secp256k1::new();
    let b = encode_commitment_set(event_id, digit, quorum, des);
    let mut r: Option<PublicKey> = None;
    let mut rhos = Vec::with_capacity(quorum.len());
    for (&j, (d_pt, e_pt)) in quorum.iter().zip(des) {
        let rho = binding_factor(j, &b);
        let term = d_pt.combine(&e_pt.mul_tweak(&secp, &Scalar::from(rho))?)?;
        r = Some(match r {
            None => term,
            Some(acc) => acc.combine(&term)?,
        });
        rhos.push(rho);
    }
    Ok((r.ok_or(secp256k1::Error::InvalidPublicKeySum)?, rhos))
}

fn check_quorum(quorum: &[u16], n: u16, t: u16) -> Result<(), FrostError> {
    if quorum.len() < t as usize {
        return Err(FrostError::QuorumTooSmall);
    }
    for (i, &j) in quorum.iter().enumerate() {
        if j == 0 || j > n || quorum[..i].contains(&j) {
            return Err(FrostError::BadQuorum);
        }
    }
    Ok(())
}

// ===================== DKG: per-participant =====================

fn pok_message(index: u16, c0: &PublicKey) -> [u8; 32] {
    let mut b = index.to_be_bytes().to_vec();
    b.extend_from_slice(&c0.serialize());
    tagged_hash(POK_TAG, &b)
}

/// Round-1 broadcast: a participant's polynomial commitment + a Schnorr
/// proof-of-knowledge of its constant term (binds the commitment to the
/// participant, defeating rogue-key attacks).
#[derive(Clone, Debug)]
pub struct Round1Package {
    pub index: u16,
    pub commitment: Vec<[u8; 33]>,
    pub pok: [u8; 64],
}

/// Round-2 point-to-point: the dealer `from`'s share of its polynomial for
/// recipient `to` (sent privately).
#[derive(Clone, Debug)]
pub struct Round2Share {
    pub from: u16,
    pub to: u16,
    pub share: [u8; 32],
}

/// A participant's long-term DKG output: its secret share `x` and the group
/// key everyone shares. `x` never leaves the participant; the rest is public.
#[derive(Clone, Debug)]
pub struct KeyShare {
    pub index: u16,
    pub n: u16,
    pub t: u16,
    pub x: [u8; 32],
    pub group_pubkey: [u8; 32],
    pub pk_odd: bool,
}

/// A participant during DKG — holds ONLY its own polynomial.
pub struct Dealer {
    index: u16,
    seed: [u8; 32],
    poly: Vec<SecretKey>,
    commitment: Vec<PublicKey>,
    pok: [u8; 64],
}

impl Dealer {
    /// Create participant `index` of an `n`-party, threshold-`t` DKG. The
    /// polynomial + nonce seed are derived from `local_seed` (real
    /// participants sample randomly; deterministic derivation makes the
    /// daemon restart-safe).
    pub fn new(index: u16, n: u16, t: u16, local_seed: &[u8; 32]) -> Result<Self, FrostError> {
        if t == 0 || n == 0 || t > n || index == 0 || index > n {
            return Err(FrostError::BadParams);
        }
        let secp = Secp256k1::new();
        let mut sb = local_seed.to_vec();
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
        let pok = crate::schnorr::sign_with_nonce(
            &poly[0].secret_bytes(),
            &tagged_hash(POK_NONCE_TAG, &seed),
            &pok_message(index, &commitment[0]),
        )?;
        Ok(Dealer { index, seed, poly, commitment, pok })
    }

    pub fn index(&self) -> u16 {
        self.index
    }

    /// This participant's nonce seed (kept secret; derives signing nonces).
    pub fn nonce_seed(&self) -> [u8; 32] {
        self.seed
    }

    /// The round-1 broadcast.
    pub fn round1(&self) -> Round1Package {
        Round1Package {
            index: self.index,
            commitment: self.commitment.iter().map(|c| c.serialize()).collect(),
            pok: self.pok,
        }
    }

    /// The private round-2 share for recipient `to` = `f_index(to)`.
    pub fn round2_share(&self, to: u16) -> Result<Round2Share, FrostError> {
        let share = poly_eval(&self.poly, &index_scalar(to))?;
        Ok(Round2Share { from: self.index, to, share: share.secret_bytes() })
    }
}

/// Verify a round-1 package: its proof-of-knowledge binds the constant-term
/// commitment to the claimed participant index.
pub fn verify_round1(pkg: &Round1Package) -> bool {
    if pkg.commitment.is_empty() {
        return false;
    }
    let Ok(c0) = PublicKey::from_byte_array_compressed(pkg.commitment[0]) else {
        return false;
    };
    let secp = Secp256k1::new();
    secp.verify_schnorr(
        &secp256k1::schnorr::Signature::from_byte_array(pkg.pok),
        &pok_message(pkg.index, &c0),
        &c0.x_only_public_key().0,
    )
    .is_ok()
}

/// Verify a dealt share against the dealer's broadcast commitment:
/// `share·G == Σ_k to^k · C_k`.
pub fn verify_round2_share(share: &Round2Share, dealer_commitment: &[[u8; 33]]) -> bool {
    let secp = Secp256k1::new();
    let Ok(sk) = SecretKey::from_byte_array(share.share) else {
        return false;
    };
    let commitment: Result<Vec<PublicKey>, _> =
        dealer_commitment.iter().map(|c| PublicKey::from_byte_array_compressed(*c)).collect();
    let Ok(commitment) = commitment else {
        return false;
    };
    match eval_commitment(&commitment, &index_scalar(share.to)) {
        Ok(expected) => sk.public_key(&secp) == expected,
        Err(_) => false,
    }
}

/// Finalize participant `index`'s key: verify every round-1 PoK and every
/// dealt share against its dealer's commitment, then sum the shares into the
/// long-term secret `x` and the constant-term commitments into the group key.
pub fn finalize_key(
    index: u16,
    n: u16,
    t: u16,
    round1: &[Round1Package],
    shares_to_me: &[Round2Share],
) -> Result<KeyShare, FrostError> {
    if round1.len() != n as usize {
        return Err(FrostError::MissingMessage);
    }
    let mut x: Option<SecretKey> = None;
    let mut pk: Option<PublicKey> = None;
    // Process dealers in index order 1..=n for a deterministic group key.
    for dealer_idx in 1..=n {
        let pkg = round1
            .iter()
            .find(|p| p.index == dealer_idx)
            .ok_or(FrostError::MissingMessage)?;
        if !verify_round1(pkg) {
            return Err(FrostError::BadPok);
        }
        let share = shares_to_me
            .iter()
            .find(|s| s.from == dealer_idx && s.to == index)
            .ok_or(FrostError::MissingMessage)?;
        if !verify_round2_share(share, &pkg.commitment) {
            return Err(FrostError::BadShare);
        }
        let sk = SecretKey::from_byte_array(share.share).map_err(|_| FrostError::BadShare)?;
        x = Some(match x {
            None => sk,
            Some(acc) => acc.add_tweak(&Scalar::from(sk))?,
        });
        let c0 = PublicKey::from_byte_array_compressed(pkg.commitment[0])
            .map_err(|_| FrostError::BadShare)?;
        pk = Some(match pk {
            None => c0,
            Some(acc) => acc.combine(&c0)?,
        });
    }
    let (px, parity) = pk.ok_or(FrostError::BadParams)?.x_only_public_key();
    Ok(KeyShare {
        index,
        n,
        t,
        x: x.ok_or(FrostError::BadParams)?.secret_bytes(),
        group_pubkey: px.serialize(),
        pk_odd: parity == Parity::Odd,
    })
}

// ===================== signing: per-participant =====================

/// A participant's nonce commitments for one event: `(D_i, E_i)` per digit.
#[derive(Clone, Debug)]
pub struct NonceCommitment {
    pub index: u16,
    pub digits: Vec<([u8; 33], [u8; 33])>,
}

/// A participant's partial signature for one event: `z_i` per digit.
#[derive(Clone, Debug)]
pub struct SigShare {
    pub index: u16,
    pub digits: Vec<[u8; 32]>,
}

/// A signer: its DKG key share plus its (secret) nonce seed.
pub struct Signer {
    pub key: KeyShare,
    nonce_seed: [u8; 32],
}

impl Signer {
    pub fn new(key: KeyShare, nonce_seed: [u8; 32]) -> Self {
        Signer { key, nonce_seed }
    }

    /// Round-1 of signing: commit nonce points for every digit of the event.
    pub fn nonce_commitment(&self, unix_ts: u64) -> NonceCommitment {
        let secp = Secp256k1::new();
        let id = event_id(unix_ts);
        let digits = (0..NB_DIGITS)
            .map(|i| {
                let (d, e) = nonce_pair(&self.nonce_seed, &id, i);
                (d.public_key(&secp).serialize(), e.public_key(&secp).serialize())
            })
            .collect();
        NonceCommitment { index: self.key.index, digits }
    }

    /// Round-2 of signing: this signer's partial signature on `price_usd`,
    /// given the quorum's nonce commitments. Uses only this signer's secret
    /// share and nonce seed.
    pub fn sig_share(
        &self,
        unix_ts: u64,
        price_usd: u32,
        quorum: &[u16],
        commitments: &[NonceCommitment],
    ) -> Result<SigShare, FrostError> {
        check_quorum(quorum, self.key.n, self.key.t)?;
        let id = event_id(unix_ts);
        let px = XOnlyPublicKey::from_byte_array(self.key.group_pubkey)?;
        let pos = quorum.iter().position(|&j| j == self.key.index).ok_or(FrostError::BadQuorum)?;
        let lambda = lagrange_at_zero(self.key.index, quorum)?;
        let x = SecretKey::from_byte_array(self.key.x).map_err(|_| FrostError::BadParams)?;

        let ds = digits(price_usd);
        let mut shares = Vec::with_capacity(ds.len());
        for (i, d) in ds.iter().enumerate() {
            let digit = i as u16;
            let des = quorum_commitments(quorum, commitments, digit)?;
            let (r, rhos) = nonce_session(&id, digit, quorum, &des)?;
            let (rx, r_parity) = r.x_only_public_key();
            let r_odd = r_parity == Parity::Odd;
            let e = challenge(&rx, &px, &attestation_msg(*d));

            // Effective nonce d_self + ρ_self·e_self, negated if R is odd-Y.
            let (dn, en) = nonce_pair(&self.nonce_seed, &id, digit);
            let eff = dn.add_tweak(&Scalar::from(en.mul_tweak(&Scalar::from(rhos[pos]))?))?;
            let eff = if r_odd { eff.negate() } else { eff };

            // e·λ_j·x_j, negated when PK is odd-Y (signing under −s).
            let key_part = lambda.mul_tweak(&Scalar::from(x))?.mul_tweak(&e)?;
            let key_part = if self.key.pk_odd { key_part.negate() } else { key_part };
            shares.push(eff.add_tweak(&Scalar::from(key_part))?.secret_bytes());
        }
        Ok(SigShare { index: self.key.index, digits: shares })
    }
}

/// Pick out the quorum's `(D_i, E_i)` for `digit`, in quorum order.
fn quorum_commitments(
    quorum: &[u16],
    commitments: &[NonceCommitment],
    digit: u16,
) -> Result<Vec<DigitCommit>, FrostError> {
    quorum
        .iter()
        .map(|&j| {
            let c = commitments.iter().find(|c| c.index == j).ok_or(FrostError::MissingMessage)?;
            let (d, e) = c.digits.get(digit as usize).ok_or(FrostError::MissingMessage)?;
            let d = PublicKey::from_byte_array_compressed(*d).map_err(|_| FrostError::Crypto)?;
            let e = PublicKey::from_byte_array_compressed(*e).map_err(|_| FrostError::Crypto)?;
            Ok((d, e))
        })
        .collect()
}

/// The aggregate nonce points per digit (the announcement), computed purely
/// from the quorum's public nonce commitments.
pub fn aggregate_announcement(
    group_pubkey: &[u8; 32],
    unix_ts: u64,
    quorum: &[u16],
    commitments: &[NonceCommitment],
) -> Result<Announcement, FrostError> {
    let _ = group_pubkey;
    let id = event_id(unix_ts);
    let mut points = Vec::with_capacity(NB_DIGITS as usize);
    for i in 0..NB_DIGITS {
        let des = quorum_commitments(quorum, commitments, i)?;
        let (r, _) = nonce_session(&id, i, quorum, &des)?;
        points.push(r.x_only_public_key().0.serialize());
    }
    let descriptor = tlv::digit_descriptor(BASE, false, "USD/BTC", 0, NB_DIGITS);
    let event = tlv::oracle_event(&points, unix_ts as u32, &descriptor, &id);
    Ok(Announcement { event_id: id, nonce_points: points, tlv_bytes: event })
}

/// Combine the quorum's partial signatures into the threshold attestation:
/// each digit's `(R_i, Σ z_{j,i})` is a BIP-340 signature under the group
/// key. Pure over public inputs — no secret needed.
pub fn aggregate_attestation(
    group_pubkey: &[u8; 32],
    unix_ts: u64,
    price_usd: u32,
    quorum: &[u16],
    commitments: &[NonceCommitment],
    shares: &[SigShare],
) -> Result<Attestation, FrostError> {
    let id = event_id(unix_ts);
    let ds = digits(price_usd);
    let mut sigs = Vec::with_capacity(ds.len());
    for (i, _) in ds.iter().enumerate() {
        let digit = i as u16;
        let des = quorum_commitments(quorum, commitments, digit)?;
        let (r, _) = nonce_session(&id, digit, quorum, &des)?;

        let mut s: Option<SecretKey> = None;
        for &j in quorum {
            let share = shares.iter().find(|s| s.index == j).ok_or(FrostError::MissingMessage)?;
            let z = share.digits.get(i).ok_or(FrostError::MissingMessage)?;
            let zk = SecretKey::from_byte_array(*z).map_err(|_| FrostError::BadShare)?;
            s = Some(match s {
                None => zk,
                Some(acc) => acc.add_tweak(&Scalar::from(zk))?,
            });
        }
        let mut sig = [0u8; 64];
        sig[..32].copy_from_slice(&r.x_only_public_key().0.serialize());
        sig[32..].copy_from_slice(&s.ok_or(FrostError::QuorumTooSmall)?.secret_bytes());
        sigs.push(sig);
    }
    let outcomes: Vec<&str> = ds.iter().map(|d| if *d == 0 { "0" } else { "1" }).collect();
    let tlv_bytes = tlv::oracle_attestation(&id, group_pubkey, &sigs, &outcomes);
    Ok(Attestation { event_id: id, price_usd, signatures: sigs, tlv_bytes })
}

// ===================== single-process simulator =====================

/// A *t*-of-*n* FROST cohort, simulated in one process: it runs the full
/// distributed DKG and signing protocols above with every participant local
/// (the wire transport is the `cohortd` daemon's). Convenience for tests and
/// `satusd-rail1`.
pub struct Cohort {
    pub n: u16,
    pub t: u16,
    pub group_pubkey: [u8; 32],
    signers: Vec<Signer>, // index j stored at j-1
}

impl Cohort {
    /// Run the full Pedersen DKG with all `n` participants local.
    pub fn keygen(n: u16, t: u16, master_seed: &[u8; 32]) -> Result<Self, FrostError> {
        let dealers: Vec<Dealer> = (1..=n)
            .map(|i| Dealer::new(i, n, t, master_seed))
            .collect::<Result<_, _>>()?;
        let round1: Vec<Round1Package> = dealers.iter().map(|d| d.round1()).collect();

        let mut signers = Vec::with_capacity(n as usize);
        for d in &dealers {
            let shares_to_me: Vec<Round2Share> =
                dealers.iter().map(|dealer| dealer.round2_share(d.index())).collect::<Result<_, _>>()?;
            let key = finalize_key(d.index(), n, t, &round1, &shares_to_me)?;
            signers.push(Signer::new(key, d.nonce_seed()));
        }
        let group_pubkey = signers[0].key.group_pubkey;
        Ok(Cohort { n, t, group_pubkey, signers })
    }

    fn quorum_signers(&self, quorum: &[u16]) -> Result<Vec<&Signer>, FrostError> {
        check_quorum(quorum, self.n, self.t)?;
        quorum
            .iter()
            .map(|&j| self.signers.get((j - 1) as usize).ok_or(FrostError::BadQuorum))
            .collect()
    }

    pub fn announce(&self, unix_ts: u64, quorum: &[u16]) -> Result<Announcement, FrostError> {
        let signers = self.quorum_signers(quorum)?;
        let commits: Vec<NonceCommitment> =
            signers.iter().map(|s| s.nonce_commitment(unix_ts)).collect();
        aggregate_announcement(&self.group_pubkey, unix_ts, quorum, &commits)
    }

    pub fn attest(&self, unix_ts: u64, price_usd: u32, quorum: &[u16]) -> Result<Attestation, FrostError> {
        let signers = self.quorum_signers(quorum)?;
        let commits: Vec<NonceCommitment> =
            signers.iter().map(|s| s.nonce_commitment(unix_ts)).collect();
        let shares: Vec<SigShare> = signers
            .iter()
            .map(|s| s.sig_share(unix_ts, price_usd, quorum, &commits))
            .collect::<Result<_, _>>()?;
        aggregate_attestation(&self.group_pubkey, unix_ts, price_usd, quorum, &commits, &shares)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schnorr::anticipation_point;
    use secp256k1::schnorr::Signature;

    #[test]
    fn dkg_then_threshold_attestation_verifies_under_one_group_key() {
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
            assert_eq!(sig[..32], ann.nonce_points[i], "digit {i} aggregate nonce");
            secp.verify_schnorr(&Signature::from_byte_array(*sig), &attestation_msg(ds[i]), &px)
                .unwrap_or_else(|e| panic!("digit {i} threshold sig must verify: {e}"));
        }
    }

    #[test]
    fn consumer_contract_holds_anticipation_point_equals_secret_point() {
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
    fn distributed_dkg_with_isolated_participants() {
        // The operational shape: each Dealer holds only its own polynomial;
        // the protocol runs purely on exchanged messages. Finalize gives
        // every participant the SAME group key and a consistent share.
        let n = 5u16;
        let t = 3u16;
        let dealers: Vec<Dealer> = (1..=n).map(|i| Dealer::new(i, n, t, &[21u8; 32]).unwrap()).collect();
        let round1: Vec<Round1Package> = dealers.iter().map(|d| d.round1()).collect();
        for pkg in &round1 {
            assert!(verify_round1(pkg), "round-1 PoK must verify");
        }
        let keys: Vec<KeyShare> = (1..=n)
            .map(|j| {
                let shares: Vec<Round2Share> =
                    dealers.iter().map(|d| d.round2_share(j).unwrap()).collect();
                for s in &shares {
                    let c = &round1.iter().find(|p| p.index == s.from).unwrap().commitment;
                    assert!(verify_round2_share(s, c), "dealt share must match commitment");
                }
                finalize_key(j, n, t, &round1, &shares).unwrap()
            })
            .collect();
        // One group key for all; shares differ.
        for k in &keys {
            assert_eq!(k.group_pubkey, keys[0].group_pubkey, "one group key");
        }
        assert_ne!(keys[0].x, keys[1].x, "distinct secret shares");

        // Those isolated key shares sign a valid threshold attestation.
        let ts = 1_700_000_010u64;
        let price = 55_555u32;
        let quorum = [1u16, 2, 4];
        let signers: Vec<Signer> = keys
            .iter()
            .map(|k| Signer::new(k.clone(), dealers[(k.index - 1) as usize].nonce_seed()))
            .collect();
        let commits: Vec<NonceCommitment> =
            quorum.iter().map(|&j| signers[(j - 1) as usize].nonce_commitment(ts)).collect();
        let shares: Vec<SigShare> = quorum
            .iter()
            .map(|&j| signers[(j - 1) as usize].sig_share(ts, price, &quorum, &commits).unwrap())
            .collect();
        let att = aggregate_attestation(&keys[0].group_pubkey, ts, price, &quorum, &commits, &shares).unwrap();

        let secp = Secp256k1::new();
        let px = XOnlyPublicKey::from_byte_array(keys[0].group_pubkey).unwrap();
        let ds = digits(price);
        for (i, sig) in att.signatures.iter().enumerate() {
            secp.verify_schnorr(&Signature::from_byte_array(*sig), &attestation_msg(ds[i]), &px)
                .unwrap_or_else(|_| panic!("digit {i} must verify"));
        }
    }

    #[test]
    fn dkg_rejects_a_tampered_share_and_forged_pok() {
        let n = 4u16;
        let t = 2u16;
        let dealers: Vec<Dealer> = (1..=n).map(|i| Dealer::new(i, n, t, &[5u8; 32]).unwrap()).collect();
        let round1: Vec<Round1Package> = dealers.iter().map(|d| d.round1()).collect();

        // A tampered dealt share fails the commitment check, and finalize
        // refuses it.
        let mut shares: Vec<Round2Share> = dealers.iter().map(|d| d.round2_share(1).unwrap()).collect();
        let c1 = &round1.iter().find(|p| p.index == shares[0].from).unwrap().commitment;
        assert!(verify_round2_share(&shares[0], c1));
        shares[0].share[0] ^= 1;
        assert!(!verify_round2_share(&shares[0], c1), "tampered share must fail");
        assert!(matches!(finalize_key(1, n, t, &round1, &shares), Err(FrostError::BadShare)));

        // A forged PoK (bound to the wrong index) is rejected.
        let mut pkg = round1[0].clone();
        pkg.index = 99;
        assert!(!verify_round1(&pkg));
    }
}
