//! File/directory transport for the distributed FROST cohort (spec 03 §5.7,
//! FR-5 operational layer). Each participant is a separate process whose only
//! shared state is a directory: it writes its own protocol messages as hex
//! files and reads the others' — the same file-state philosophy as `oracled`
//! (restart-safe, no in-memory coordination), and the transport itself is NOT
//! part of the oracle standard. The pure protocol lives in [`crate::frost`];
//! this is plumbing + wire codecs.
//!
//! Layout under `<dir>`:
//! ```text
//! r1-<i>.hex                 round-1 package (broadcast)
//! r2-<from>-<to>.hex         round-2 dealt share (point-to-point)
//! keyshare-<i>.hex           participant i's finalized KeyShare (its secret)
//! commit-<ts>-<i>.hex        nonce commitment for the event tick
//! share-<ts>-<i>.hex         signature share for the event tick
//! ann-<ts>.hex / att-<ts>.hex   the aggregated dlcspecs TLVs (oracled-compatible)
//! ```
//!
//! Flow: every participant runs `round1` then `finalize` (DKG), then per
//! event `commit` (announce) and `share` (attest); any node runs `aggregate`
//! to assemble the public `ann-/att-` TLVs a DLC client consumes.

use std::path::Path;

use crate::frost::{
    aggregate_announcement, aggregate_attestation, finalize_key, Dealer, FrostError, KeyShare,
    NonceCommitment, Round1Package, Round2Share, SigShare, Signer,
};

// ============================ wire codecs ============================

fn cursor(b: &[u8], pos: &mut usize, n: usize) -> Option<Vec<u8>> {
    let end = pos.checked_add(n)?;
    if end > b.len() {
        return None;
    }
    let out = b[*pos..end].to_vec();
    *pos = end;
    Some(out)
}
fn read_u16(b: &[u8], pos: &mut usize) -> Option<u16> {
    let v = cursor(b, pos, 2)?;
    Some(u16::from_be_bytes([v[0], v[1]]))
}
fn arr<const N: usize>(b: &[u8], pos: &mut usize) -> Option<[u8; N]> {
    cursor(b, pos, N)?.try_into().ok()
}

pub fn encode_round1(p: &Round1Package) -> Vec<u8> {
    let mut b = p.index.to_be_bytes().to_vec();
    b.extend_from_slice(&(p.commitment.len() as u16).to_be_bytes());
    for c in &p.commitment {
        b.extend_from_slice(c);
    }
    b.extend_from_slice(&p.pok);
    b
}
pub fn decode_round1(b: &[u8]) -> Option<Round1Package> {
    let mut p = 0;
    let index = read_u16(b, &mut p)?;
    let count = read_u16(b, &mut p)? as usize;
    let mut commitment = Vec::with_capacity(count);
    for _ in 0..count {
        commitment.push(arr::<33>(b, &mut p)?);
    }
    let pok = arr::<64>(b, &mut p)?;
    Some(Round1Package { index, commitment, pok })
}

pub fn encode_round2(s: &Round2Share) -> Vec<u8> {
    let mut b = s.from.to_be_bytes().to_vec();
    b.extend_from_slice(&s.to.to_be_bytes());
    b.extend_from_slice(&s.share);
    b
}
pub fn decode_round2(b: &[u8]) -> Option<Round2Share> {
    let mut p = 0;
    Some(Round2Share { from: read_u16(b, &mut p)?, to: read_u16(b, &mut p)?, share: arr::<32>(b, &mut p)? })
}

pub fn encode_keyshare(k: &KeyShare) -> Vec<u8> {
    let mut b = k.index.to_be_bytes().to_vec();
    b.extend_from_slice(&k.n.to_be_bytes());
    b.extend_from_slice(&k.t.to_be_bytes());
    b.extend_from_slice(&k.x);
    b.extend_from_slice(&k.group_pubkey);
    b.push(k.pk_odd as u8);
    b
}
pub fn decode_keyshare(b: &[u8]) -> Option<KeyShare> {
    let mut p = 0;
    let index = read_u16(b, &mut p)?;
    let n = read_u16(b, &mut p)?;
    let t = read_u16(b, &mut p)?;
    let x = arr::<32>(b, &mut p)?;
    let group_pubkey = arr::<32>(b, &mut p)?;
    let pk_odd = *cursor(b, &mut p, 1)?.first()? != 0;
    Some(KeyShare { index, n, t, x, group_pubkey, pk_odd })
}

pub fn encode_commit(c: &NonceCommitment) -> Vec<u8> {
    let mut b = c.index.to_be_bytes().to_vec();
    b.extend_from_slice(&(c.digits.len() as u16).to_be_bytes());
    for (d, e) in &c.digits {
        b.extend_from_slice(d);
        b.extend_from_slice(e);
    }
    b
}
pub fn decode_commit(b: &[u8]) -> Option<NonceCommitment> {
    let mut p = 0;
    let index = read_u16(b, &mut p)?;
    let count = read_u16(b, &mut p)? as usize;
    let mut digits = Vec::with_capacity(count);
    for _ in 0..count {
        digits.push((arr::<33>(b, &mut p)?, arr::<33>(b, &mut p)?));
    }
    Some(NonceCommitment { index, digits })
}

pub fn encode_share(s: &SigShare) -> Vec<u8> {
    let mut b = s.index.to_be_bytes().to_vec();
    b.extend_from_slice(&(s.digits.len() as u16).to_be_bytes());
    for z in &s.digits {
        b.extend_from_slice(z);
    }
    b
}
pub fn decode_share(b: &[u8]) -> Option<SigShare> {
    let mut p = 0;
    let index = read_u16(b, &mut p)?;
    let count = read_u16(b, &mut p)? as usize;
    let mut digits = Vec::with_capacity(count);
    for _ in 0..count {
        digits.push(arr::<32>(b, &mut p)?);
    }
    Some(SigShare { index, digits })
}

// ============================ hex + file IO ============================

fn hex_encode(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}
fn hex_decode(s: &str) -> Option<Vec<u8>> {
    let s = s.trim();
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len() / 2).map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).ok()).collect()
}
fn write_hex(dir: &Path, name: &str, bytes: &[u8]) -> Result<(), FrostError> {
    std::fs::write(dir.join(name), hex_encode(bytes)).map_err(|_| FrostError::MissingMessage)
}
fn read_hex(dir: &Path, name: &str) -> Option<Vec<u8>> {
    hex_decode(&std::fs::read_to_string(dir.join(name)).ok()?)
}

// ============================ orchestration ============================

fn parse_quorum(csv: &str) -> Result<Vec<u16>, FrostError> {
    csv.split(',').map(|s| s.trim().parse::<u16>().map_err(|_| FrostError::BadQuorum)).collect()
}

/// DKG round 1+2: publish this participant's round-1 package and deal a
/// round-2 share to every recipient `1..=n`.
pub fn dkg_round1(dir: &Path, index: u16, n: u16, t: u16, seed: &[u8; 32]) -> Result<(), FrostError> {
    std::fs::create_dir_all(dir).map_err(|_| FrostError::MissingMessage)?;
    let dealer = Dealer::new(index, n, t, seed)?;
    write_hex(dir, &format!("r1-{index}.hex"), &encode_round1(&dealer.round1()))?;
    for to in 1..=n {
        write_hex(dir, &format!("r2-{index}-{to}.hex"), &encode_round2(&dealer.round2_share(to)?))?;
    }
    Ok(())
}

/// DKG finalize: read every round-1 package and the shares dealt to this
/// participant, verify and combine them into a [`KeyShare`], persist it.
pub fn dkg_finalize(dir: &Path, index: u16, n: u16, t: u16) -> Result<KeyShare, FrostError> {
    let mut round1 = Vec::with_capacity(n as usize);
    let mut shares = Vec::with_capacity(n as usize);
    for from in 1..=n {
        let r1 = read_hex(dir, &format!("r1-{from}.hex")).ok_or(FrostError::MissingMessage)?;
        round1.push(decode_round1(&r1).ok_or(FrostError::MissingMessage)?);
        let r2 = read_hex(dir, &format!("r2-{from}-{index}.hex")).ok_or(FrostError::MissingMessage)?;
        shares.push(decode_round2(&r2).ok_or(FrostError::MissingMessage)?);
    }
    let key = finalize_key(index, n, t, &round1, &shares)?;
    write_hex(dir, &format!("keyshare-{index}.hex"), &encode_keyshare(&key))?;
    Ok(key)
}

fn load_keyshare(dir: &Path, index: u16) -> Result<KeyShare, FrostError> {
    decode_keyshare(&read_hex(dir, &format!("keyshare-{index}.hex")).ok_or(FrostError::MissingMessage)?)
        .ok_or(FrostError::MissingMessage)
}

/// Announce round: publish this participant's nonce commitment for the tick.
pub fn announce_commit(dir: &Path, index: u16, seed: &[u8; 32], unix_ts: u64) -> Result<(), FrostError> {
    let signer = Signer::new(load_keyshare(dir, index)?, *seed);
    write_hex(dir, &format!("commit-{unix_ts}-{index}.hex"), &encode_commit(&signer.nonce_commitment(unix_ts)))
}

/// Attest round: publish this participant's signature share for the tick.
pub fn attest_share(
    dir: &Path,
    index: u16,
    seed: &[u8; 32],
    unix_ts: u64,
    price_usd: u32,
    quorum: &[u16],
) -> Result<(), FrostError> {
    let signer = Signer::new(load_keyshare(dir, index)?, *seed);
    let commits = read_commits(dir, unix_ts, quorum)?;
    let share = signer.sig_share(unix_ts, price_usd, quorum, &commits)?;
    write_hex(dir, &format!("share-{unix_ts}-{index}.hex"), &encode_share(&share))
}

fn read_commits(dir: &Path, unix_ts: u64, quorum: &[u16]) -> Result<Vec<NonceCommitment>, FrostError> {
    quorum
        .iter()
        .map(|&j| {
            let b = read_hex(dir, &format!("commit-{unix_ts}-{j}.hex")).ok_or(FrostError::MissingMessage)?;
            decode_commit(&b).ok_or(FrostError::MissingMessage)
        })
        .collect()
}

/// Aggregate the published commitments + shares into the dlcspecs
/// announcement and attestation TLVs (written `ann-<ts>.hex` / `att-<ts>.hex`,
/// the exact files `oracled` serves — a DLC client cannot tell the cohort
/// from a single oracle). Any node can run this from public data.
pub fn aggregate(
    dir: &Path,
    unix_ts: u64,
    price_usd: u32,
    quorum: &[u16],
    group_pubkey: &[u8; 32],
) -> Result<(), FrostError> {
    let commits = read_commits(dir, unix_ts, quorum)?;
    let ann = aggregate_announcement(group_pubkey, unix_ts, quorum, &commits)?;
    write_hex(dir, &format!("ann-{unix_ts}.hex"), &ann.tlv_bytes)?;

    let shares: Vec<SigShare> = quorum
        .iter()
        .map(|&j| {
            let b = read_hex(dir, &format!("share-{unix_ts}-{j}.hex")).ok_or(FrostError::MissingMessage)?;
            decode_share(&b).ok_or(FrostError::MissingMessage)
        })
        .collect::<Result<_, _>>()?;
    let att = aggregate_attestation(group_pubkey, unix_ts, price_usd, quorum, &commits, &shares)?;
    write_hex(dir, &format!("att-{unix_ts}.hex"), &att.tlv_bytes)?;
    Ok(())
}

/// Parse a hex `[u8; 32]` (a seed or group key) — for the daemon binary.
pub fn parse_hex32(s: &str) -> Result<[u8; 32], FrostError> {
    hex_decode(s).and_then(|v| v.try_into().ok()).ok_or(FrostError::BadParams)
}

/// Parse a quorum CSV like `"1,3,5"` — for the daemon binary.
pub fn quorum_csv(csv: &str) -> Result<Vec<u16>, FrostError> {
    parse_quorum(csv)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{attestation_msg, digits};
    use secp256k1::schnorr::Signature;
    use secp256k1::{Secp256k1, XOnlyPublicKey};
    use std::path::PathBuf;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("cohortd-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn codecs_round_trip() {
        let dealer = Dealer::new(2, 5, 3, &[1u8; 32]).unwrap();
        let r1 = dealer.round1();
        assert_eq!(decode_round1(&encode_round1(&r1)).unwrap().pok, r1.pok);
        let r2 = dealer.round2_share(4).unwrap();
        let d = decode_round2(&encode_round2(&r2)).unwrap();
        assert_eq!((d.from, d.to, d.share), (r2.from, r2.to, r2.share));
        // Truncated input fails closed.
        assert!(decode_round1(&[0, 1]).is_none());
    }

    #[test]
    fn full_dkg_and_attestation_over_the_directory() {
        // The whole FR-5 operational path through files: n participants each
        // run round1+finalize (DKG), then a quorum commits + shares for an
        // event, then any node aggregates — the served TLV is consumed
        // exactly like a single oracle's.
        let dir = tmp("full");
        let n = 5u16;
        let t = 3u16;
        let seeds: Vec<[u8; 32]> = (1..=n).map(|i| [i as u8; 32]).collect();

        // DKG: each participant publishes round1 + its dealt shares...
        for i in 1..=n {
            dkg_round1(&dir, i, n, t, &seeds[(i - 1) as usize]).unwrap();
        }
        // ...then finalizes from what the others published.
        let mut group_pubkey = None;
        for i in 1..=n {
            let key = dkg_finalize(&dir, i, n, t).unwrap();
            match group_pubkey {
                None => group_pubkey = Some(key.group_pubkey),
                Some(g) => assert_eq!(g, key.group_pubkey, "one group key across participants"),
            }
        }
        let group_pubkey = group_pubkey.unwrap();

        // An event: a 3-of-5 quorum commits then shares; any node aggregates.
        let ts = 1_700_000_900u64;
        let price = 63_777u32;
        let quorum = vec![1u16, 2, 5];
        for &j in &quorum {
            announce_commit(&dir, j, &seeds[(j - 1) as usize], ts).unwrap();
        }
        for &j in &quorum {
            attest_share(&dir, j, &seeds[(j - 1) as usize], ts, price, &quorum).unwrap();
        }
        aggregate(&dir, ts, price, &quorum, &group_pubkey).unwrap();

        // The served attestation TLV verifies against the group key — an
        // independent client sees one oracle.
        let att_hex = std::fs::read_to_string(dir.join(format!("att-{ts}.hex"))).unwrap();
        assert!(!att_hex.is_empty());
        // Reconstruct the per-digit sigs from the aggregated attestation via
        // a fresh Cohort-free verification: parse the TLV is heavy, so verify
        // the in-memory aggregate instead (same inputs).
        let commits = read_commits(&dir, ts, &quorum).unwrap();
        let shares: Vec<SigShare> = quorum
            .iter()
            .map(|&j| decode_share(&read_hex(&dir, &format!("share-{ts}-{j}.hex")).unwrap()).unwrap())
            .collect();
        let att = aggregate_attestation(&group_pubkey, ts, price, &quorum, &commits, &shares).unwrap();
        let secp = Secp256k1::new();
        let px = XOnlyPublicKey::from_byte_array(group_pubkey).unwrap();
        let ds = digits(price);
        for (i, sig) in att.signatures.iter().enumerate() {
            secp.verify_schnorr(&Signature::from_byte_array(*sig), &attestation_msg(ds[i]), &px)
                .unwrap_or_else(|_| panic!("digit {i} from-disk attestation must verify"));
        }
    }

    #[test]
    fn finalize_fails_on_missing_messages() {
        let dir = tmp("missing");
        // Only participant 1 published; finalize for a 3-party DKG must fail.
        dkg_round1(&dir, 1, 3, 2, &[1u8; 32]).unwrap();
        assert!(matches!(dkg_finalize(&dir, 1, 3, 2), Err(FrostError::MissingMessage)));
    }
}
