//! Bitcoin SPV verification of a `BtcPayoutConfirmation` (PRD §5.D14, DL-22).
//!
//! Proves, in pure Rust, that the user's BTC HTLC claim spend is buried under
//! ≥ `BTC_CLAIM_CONFIRMATION_DEPTH` confirmations — the resource-safety point
//! that lets a reserve reimbursement claim finalize. Replaces the
//! `confirmation_depth`/`revealed_preimage` facts the redeem verifier trusted.
//!
//! Checks (the off-chain-verifiable subset of §5.D14): the revealed preimage
//! hashes to the payment hash (2); the htlc + claim txs are merkle-included in
//! their stated block headers (5,6); the claim block is no earlier than the htlc
//! block (7); ≥ K confirmation headers chain onto the claim block, each with
//! valid PoW (8,9); and the claim block is buried ≥ K deep vs the tip (10).
//!
//! Not covered (needs more witness data, tracked as follow-ups): that the claim
//! tx actually spends the HTLC outpoint (R-15, needs the claim tx body) and that
//! the HTLC output script commits the right payment_hash/pubkey/CSV (needs script
//! parsing).

use satusd_types::sha256;
use satusd_types::types::BtcPayoutConfirmation;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SpvError {
    PreimageHashMismatch,
    HtlcHeaderMismatch,
    ClaimHeaderMismatch,
    HtlcMerkleInvalid,
    ClaimMerkleInvalid,
    ClaimBeforeHtlc,
    InsufficientConfirmations, // R-07, R-13
    HeaderChainBroken,         // R-14
    HeaderPowInvalid,          // R-14
    NotBuriedDeepEnough,
}

/// Bitcoin double-SHA256.
fn dsha256(b: &[u8]) -> [u8; 32] {
    sha256(&[&sha256(&[b])])
}

/// Block hash (internal byte order) of an 80-byte header.
fn block_hash(header: &[u8; 80]) -> [u8; 32] {
    dsha256(header)
}

fn prev_block_hash(header: &[u8; 80]) -> [u8; 32] {
    header[4..36].try_into().unwrap()
}

fn merkle_root_field(header: &[u8; 80]) -> [u8; 32] {
    header[36..68].try_into().unwrap()
}

/// Expand the compact `nBits` (header bytes 72..76, LE) to a 32-byte big-endian
/// target.
fn target_from_nbits(nbits: u32) -> [u8; 32] {
    let exp = (nbits >> 24) as i32;
    let mant = nbits & 0x007f_ffff; // mask the sign bit
    let mut t = [0u8; 32];
    for i in 0..3i32 {
        let byte = ((mant >> (8 * i)) & 0xff) as u8;
        let pos = 34 - exp - i; // big-endian index of this mantissa byte
        if (0..32).contains(&pos) {
            t[pos as usize] = byte;
        }
    }
    t
}

/// Proof-of-work: block hash (as a big-endian integer) ≤ target.
fn pow_ok(header: &[u8; 80]) -> bool {
    let nbits = u32::from_le_bytes(header[72..76].try_into().unwrap());
    let target = target_from_nbits(nbits);
    let mut be = block_hash(header); // internal (LE); reverse to big-endian
    be.reverse();
    be <= target
}

/// Fold a leaf up a Bitcoin merkle branch using the tx `index` for direction.
fn merkle_root_from_proof(leaf: [u8; 32], mut index: u32, branch: &[[u8; 32]]) -> [u8; 32] {
    let mut h = leaf;
    for sib in branch {
        let mut buf = [0u8; 64];
        if index & 1 == 0 {
            buf[..32].copy_from_slice(&h);
            buf[32..].copy_from_slice(sib);
        } else {
            buf[..32].copy_from_slice(sib);
            buf[32..].copy_from_slice(&h);
        }
        h = dsha256(&buf);
        index >>= 1;
    }
    h
}

/// Verify a `BtcPayoutConfirmation` proves the claim spend is buried ≥ `min_depth`
/// confirmations. `tip_height` is the best-chain tip (from local bitcoind).
pub fn verify_payout_confirmation(
    c: &BtcPayoutConfirmation,
    payment_hash: &[u8; 32],
    min_depth: usize,
    tip_height: u32,
) -> Result<(), SpvError> {
    use SpvError::*;

    macro_rules! ensure {
        ($cond:expr, $err:expr) => {
            if !($cond) {
                return Err($err);
            }
        };
    }

    // (2) Revealed preimage hashes to the payment hash.
    ensure!(
        sha256(&[&c.revealed_preimage]) == *payment_hash,
        PreimageHashMismatch
    );

    // Inclusion headers are consistent with the stated block hashes, and have PoW.
    ensure!(
        block_hash(&c.htlc_inclusion_header) == c.htlc_inclusion_block_hash,
        HtlcHeaderMismatch
    );
    ensure!(
        block_hash(&c.claim_inclusion_header) == c.claim_inclusion_block_hash,
        ClaimHeaderMismatch
    );
    ensure!(pow_ok(&c.htlc_inclusion_header), HeaderPowInvalid);
    ensure!(pow_ok(&c.claim_inclusion_header), HeaderPowInvalid);

    // (5,6) Merkle inclusion of both txs in their blocks.
    ensure!(
        merkle_root_from_proof(
            c.btc_htlc_txid,
            c.htlc_tx_index,
            &c.htlc_inclusion_merkle_proof
        ) == merkle_root_field(&c.htlc_inclusion_header),
        HtlcMerkleInvalid
    );
    ensure!(
        merkle_root_from_proof(
            c.claim_spend_txid,
            c.claim_tx_index,
            &c.claim_inclusion_merkle_proof
        ) == merkle_root_field(&c.claim_inclusion_header),
        ClaimMerkleInvalid
    );

    // (7) Claim is no earlier than the HTLC.
    ensure!(
        c.claim_inclusion_block_height >= c.htlc_inclusion_block_height,
        ClaimBeforeHtlc
    );

    // (8) Enough confirmation headers. (R-07 / R-13)
    ensure!(
        c.confirmation_headers.len() >= min_depth,
        InsufficientConfirmations
    );

    // (9) They chain onto the claim block, each with valid PoW. (R-14)
    let mut prev = c.claim_inclusion_block_hash;
    for hdr in &c.confirmation_headers {
        ensure!(prev_block_hash(hdr) == prev, HeaderChainBroken);
        ensure!(pow_ok(hdr), HeaderPowInvalid);
        prev = block_hash(hdr);
    }

    // (10) Buried ≥ min_depth vs the tip.
    let needed = c
        .claim_inclusion_block_height
        .saturating_add(min_depth as u32);
    ensure!(tip_height >= needed, NotBuriedDeepEnough);
    Ok(())
}

/// Test-only: build a valid regtest confirmation for `preimage`, with the given
/// HTLC output value and claim buried under `depth` headers at `claim_height`.
/// Shared by the SPV tests and the batch ReserveClaim tests (`claim.rs`).
#[cfg(test)]
pub(crate) fn build_confirmation(
    preimage: [u8; 32],
    payout_sats: u64,
    claim_height: u32,
    depth: usize,
) -> BtcPayoutConfirmation {
    /// Grind a regtest-difficulty header (nBits 0x207fffff) until PoW holds.
    fn header(prev: [u8; 32], merkle: [u8; 32]) -> [u8; 80] {
        let mut h = [0u8; 80];
        h[0..4].copy_from_slice(&1u32.to_le_bytes());
        h[4..36].copy_from_slice(&prev);
        h[36..68].copy_from_slice(&merkle);
        h[68..72].copy_from_slice(&1_700_000_000u32.to_le_bytes());
        h[72..76].copy_from_slice(&0x207f_ffffu32.to_le_bytes());
        for n in 0u32..u32::MAX {
            h[76..80].copy_from_slice(&n.to_le_bytes());
            if pow_ok(&h) {
                return h;
            }
        }
        panic!("no nonce");
    }
    // Distinct txids per preimage so batched confirmations don't collide.
    let htlc_txid = sha256(&[&b"htlc"[..], &preimage[..]]);
    let claim_txid = sha256(&[&b"claim"[..], &preimage[..]]);
    let htlc_sib = [0xa1; 32];
    let claim_sib = [0xb2; 32];
    let htlc_root = merkle_root_from_proof(htlc_txid, 0, &[htlc_sib]);
    let claim_root = merkle_root_from_proof(claim_txid, 0, &[claim_sib]);
    let htlc_hdr = header([0; 32], htlc_root);
    let claim_hdr = header([0x99; 32], claim_root);
    let claim_bh = block_hash(&claim_hdr);

    let mut confs = Vec::new();
    let mut prev = claim_bh;
    for _ in 0..depth {
        let h = header(prev, [0x33; 32]);
        prev = block_hash(&h);
        confs.push(h);
    }

    BtcPayoutConfirmation {
        btc_htlc_txid: htlc_txid,
        btc_htlc_vout: 0,
        htlc_output_value_sats: payout_sats,
        htlc_output_script: vec![],
        htlc_inclusion_block_hash: block_hash(&htlc_hdr),
        htlc_inclusion_block_height: claim_height - 1,
        htlc_inclusion_merkle_proof: vec![htlc_sib],
        claim_spend_txid: claim_txid,
        claim_spend_input_index: 0,
        claim_spend_witness: vec![preimage.to_vec()],
        revealed_preimage: preimage,
        claim_inclusion_block_hash: claim_bh,
        claim_inclusion_block_height: claim_height,
        claim_inclusion_merkle_proof: vec![claim_sib],
        confirmation_headers: confs,
        htlc_tx_index: 0,
        claim_tx_index: 0,
        htlc_inclusion_header: htlc_hdr,
        claim_inclusion_header: claim_hdr,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PREIMAGE: [u8; 32] = [0x55; 32];

    fn payment_hash() -> [u8; 32] {
        sha256(&[&PREIMAGE])
    }

    fn good_confirmation(depth: usize) -> BtcPayoutConfirmation {
        build_confirmation(PREIMAGE, 100_000, 101, depth)
    }

    #[test]
    fn happy_path() {
        let c = good_confirmation(6);
        verify_payout_confirmation(&c, &payment_hash(), 6, 200).expect("spv ok");
    }

    #[test]
    fn r07_r13_insufficient_confirmations() {
        let c = good_confirmation(5); // < 6
        assert_eq!(
            verify_payout_confirmation(&c, &payment_hash(), 6, 200),
            Err(SpvError::InsufficientConfirmations)
        );
    }

    #[test]
    fn r14_broken_chain() {
        let mut c = good_confirmation(6);
        // Corrupt the link from the claim block to the first confirmation header.
        c.confirmation_headers[0][4] ^= 0xff;
        assert!(matches!(
            verify_payout_confirmation(&c, &payment_hash(), 6, 200),
            Err(SpvError::HeaderChainBroken) | Err(SpvError::HeaderPowInvalid)
        ));
    }

    #[test]
    fn r14_bad_merkle_proof() {
        let mut c = good_confirmation(6);
        c.claim_inclusion_merkle_proof[0][0] ^= 0xff; // wrong sibling
        assert_eq!(
            verify_payout_confirmation(&c, &payment_hash(), 6, 200),
            Err(SpvError::ClaimMerkleInvalid)
        );
    }

    #[test]
    fn wrong_preimage_rejected() {
        let mut c = good_confirmation(6);
        c.revealed_preimage = [0x00; 32];
        assert_eq!(
            verify_payout_confirmation(&c, &payment_hash(), 6, 200),
            Err(SpvError::PreimageHashMismatch)
        );
    }

    #[test]
    fn not_buried_vs_tip() {
        let c = good_confirmation(6);
        // tip only at the claim block height ⇒ not 6 deep.
        assert_eq!(
            verify_payout_confirmation(&c, &payment_hash(), 6, 101),
            Err(SpvError::NotBuriedDeepEnough)
        );
    }
}
