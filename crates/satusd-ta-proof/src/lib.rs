//! Taproot Asset proof verifier (tapd v0.7.2 format) for DL-23 lineage
//! enforcement (PRD §5.D15). State node must independently re-verify the full TA
//! lineage from genesis to the lock output — `asset_proof_hash` is not a trust
//! anchor. This is the Rust port of `tapd`'s proof validation; it is built and
//! cross-checked against real proofs exported from a live tapd via `ExportProof`,
//! with tapd's own `VerifyProof` as the ground-truth oracle.
//!
//! Implemented (validated byte-for-byte against live tapd):
//! - `proof.File` container: `"TAPF" || u32 ver || BigSize count || count×[ BigSize
//!   len || proof || sha256-chain checksum ]` ([`parse_proof_file`]).
//! - `proof.Proof` TLV decode ([`parse_proof`]) + asset-leaf / asset_id
//!   ([`AssetLeaf`], [`Genesis::asset_id`]).
//! - On-chain anchoring ([`verify_anchor`]): AnchorTx (rust-bitcoin) + TxMerkleProof
//!   → block header merkle root; extracts the P2TR output key.
//! - Asset→output-key commitment ([`verify_asset_inclusion`]): asset →
//!   AssetCommitment → TapCommitment MS-SMTs → V2 tapscript leaf → taptweak, must
//!   equal the on-chain output key (DL-23 inclusion). Works for both mint outputs
//!   and transfer (split) outputs — split outputs commit the asset leaf with the
//!   split-commitment witness stripped, and the MS-SMT folds use the proof's own
//!   siblings (general compressed proofs, not an assumed single-leaf path).
//!   Also supports an optional tapscript sibling (lock anchors): the output's
//!   tapscript root is the BIP341 `TapBranch` of the TA-commitment leaf with the
//!   sibling preimage's tap hash. Handles grouped (reissuable) assets — the inner
//!   AssetCommitment is keyed by `sha256(asset_id || xonly(scriptkey))`.
//! - Lineage continuity ([`verify_lineage`]): every step's asset is committed in
//!   its anchor output AND `prev_out` chains to the prior step's anchored output.
//!
//! The §5.D15 lineage→lock bindings consume this verifier in
//! `satusd-state::redeem`. Not needed for SatUSD (single asset per
//! lock/finalize output): multi-asset commitments, V0/V1 TapCommitments.

use sha2::{Digest, Sha256};

pub mod mssmt;

/// `proof.File` magic ("TAPF").
pub const FILE_MAGIC: [u8; 4] = *b"TAPF";
/// `proof.Proof` magic ("TAPP").
pub const PROOF_MAGIC: [u8; 4] = *b"TAPP";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProofError {
    Truncated,
    BadFileMagic,
    BadProofMagic,
    /// Hash-chain checksum did not match (corrupted / reordered proofs).
    ChecksumMismatch,
    TrailingBytes,
    /// BigSize used a longer encoding than necessary (tapd rejects non-canonical).
    NonCanonicalBigSize,
    /// TLV record types must be strictly ascending.
    TlvOutOfOrder,
    /// Anchor tx is not consensus-decodable.
    BadAnchorTx,
    /// Tx-merkle proof does not reconstruct the block header's merkle root.
    AnchorMerkleMismatch,
    /// TaprootProof output index is out of range for the anchor tx.
    OutputIndexOutOfRange,
    /// The referenced anchor output is not a P2TR (v1 witness) output.
    NotP2tr,
    /// A field had an unexpected length.
    BadFieldLength,
    /// The reconstructed taproot output key ≠ the on-chain anchored output key —
    /// the asset is NOT committed in the anchor output (fake lineage, DL-23).
    CommitmentMismatch,
    /// Only V2 TapCommitments are supported by the reconstruction so far.
    UnsupportedTapVersion,
    /// A proof step's input (prev_out) is not the previous step's anchored output.
    LineageBroken,
    EmptyLineage,
    /// A tapscript sibling preimage had an unknown type or invalid length.
    BadTapSibling,
}

/// Top-level `proof.Proof` TLV type numbers (tapd v0.7.2 `proof/records.go`).
pub mod tlv {
    pub const VERSION: u64 = 0;
    pub const PREV_OUT: u64 = 2;
    pub const BLOCK_HEADER: u64 = 4;
    pub const ANCHOR_TX: u64 = 6;
    pub const TX_MERKLE_PROOF: u64 = 8;
    pub const ASSET_LEAF: u64 = 10;
    pub const INCLUSION_PROOF: u64 = 12;
    pub const EXCLUSION_PROOFS: u64 = 13;
    pub const SPLIT_ROOT_PROOF: u64 = 15;
    pub const META_REVEAL: u64 = 17;
    pub const ADDITIONAL_INPUTS: u64 = 19;
    pub const CHALLENGE_WITNESS: u64 = 21;
    pub const BLOCK_HEIGHT: u64 = 22;
    pub const GENESIS_REVEAL: u64 = 23;
    pub const GROUP_KEY_REVEAL: u64 = 25;
    pub const ALT_LEAVES: u64 = 27;
}

/// A parsed `proof.File`: the file version plus the raw bytes of each contained
/// single proof (each starts with `TAPP`), in lineage order (genesis first).
#[derive(Debug, PartialEq, Eq)]
pub struct ProofFile<'a> {
    pub version: u32,
    pub proofs: Vec<&'a [u8]>,
}

struct Cursor<'a> {
    b: &'a [u8],
    i: usize,
}

impl<'a> Cursor<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], ProofError> {
        let end = self.i.checked_add(n).ok_or(ProofError::Truncated)?;
        let s = self.b.get(self.i..end).ok_or(ProofError::Truncated)?;
        self.i = end;
        Ok(s)
    }

    fn u32_be(&mut self) -> Result<u32, ProofError> {
        Ok(u32::from_be_bytes(self.take(4)?.try_into().unwrap()))
    }

    /// lnd BigSize varint: 1/3/5/9 bytes, big-endian, canonical (shortest) form.
    fn bigsize(&mut self) -> Result<u64, ProofError> {
        let first = self.take(1)?[0];
        let v = match first {
            0xff => {
                let x = u64::from_be_bytes(self.take(8)?.try_into().unwrap());
                if x < 0x1_0000_0000 {
                    return Err(ProofError::NonCanonicalBigSize);
                }
                x
            }
            0xfe => {
                let x = u32::from_be_bytes(self.take(4)?.try_into().unwrap()) as u64;
                if x < 0x1_0000 {
                    return Err(ProofError::NonCanonicalBigSize);
                }
                x
            }
            0xfd => {
                let x = u16::from_be_bytes(self.take(2)?.try_into().unwrap()) as u64;
                if x < 0xfd {
                    return Err(ProofError::NonCanonicalBigSize);
                }
                x
            }
            n => n as u64,
        };
        Ok(v)
    }
}

/// Parse a tapd `proof.File`, verifying its sha256 hash chain. Returns the file
/// version and each contained single-proof blob in order.
pub fn parse_proof_file(data: &[u8]) -> Result<ProofFile<'_>, ProofError> {
    let mut c = Cursor { b: data, i: 0 };
    if c.take(4)? != FILE_MAGIC {
        return Err(ProofError::BadFileMagic);
    }
    let version = c.u32_be()?;
    let count = c.bigsize()?;

    let mut proofs = Vec::with_capacity(count as usize);
    let mut prev_hash = [0u8; 32];
    for _ in 0..count {
        let len = c.bigsize()? as usize;
        let proof = c.take(len)?;
        if proof.get(0..4) != Some(&PROOF_MAGIC[..]) {
            return Err(ProofError::BadProofMagic);
        }
        let checksum = c.take(32)?;
        // tapd hash chain: h_i = sha256(h_{i-1} || proof_i), h_{-1} = 0^32.
        let mut hasher = Sha256::new();
        hasher.update(prev_hash);
        hasher.update(proof);
        let h = hasher.finalize();
        if h.as_slice() != checksum {
            return Err(ProofError::ChecksumMismatch);
        }
        prev_hash.copy_from_slice(&h);
        proofs.push(proof);
    }
    if c.i != data.len() {
        return Err(ProofError::TrailingBytes);
    }
    Ok(ProofFile { version, proofs })
}

impl<'a> ProofFile<'a> {
    /// Parse every contained single proof's TLV stream (genesis first).
    pub fn parsed(&self) -> Result<Vec<Proof<'a>>, ProofError> {
        self.proofs.iter().map(|p| parse_proof(p)).collect()
    }
}

/// A single `proof.Proof`: `"TAPP"` + a strictly-ascending TLV stream. Records
/// are kept generically (type, raw value); nested fields (asset leaf, anchor tx,
/// inclusion/split proofs) are decoded in later increments.
#[derive(Debug, PartialEq, Eq)]
pub struct Proof<'a> {
    pub records: Vec<(u64, &'a [u8])>,
}

impl<'a> Proof<'a> {
    /// The raw value of the given TLV type, if present.
    pub fn get(&self, ty: u64) -> Option<&'a [u8]> {
        self.records.iter().find(|(t, _)| *t == ty).map(|(_, v)| *v)
    }

    fn u32_field(&self, ty: u64) -> Option<u32> {
        self.get(ty)
            .and_then(|v| <[u8; 4]>::try_from(v).ok())
            .map(u32::from_be_bytes)
    }

    pub fn version(&self) -> Option<u32> {
        self.u32_field(tlv::VERSION)
    }

    /// Bitcoin block height anchoring this proof's transition.
    pub fn block_height(&self) -> Option<u32> {
        self.u32_field(tlv::BLOCK_HEIGHT)
    }

    /// Decode this proof's asset leaf (`ASSET_LEAF`, type 10).
    pub fn asset_leaf(&self) -> Result<AssetLeaf<'a>, ProofError> {
        parse_asset_leaf(self.get(tlv::ASSET_LEAF).ok_or(ProofError::Truncated)?)
    }

    /// Re-encode to the canonical proof blob (`"TAPP"` + TLV), for round-trip
    /// checks — a lossless parse must reproduce the original bytes.
    pub fn reencode(&self) -> Vec<u8> {
        let mut out = PROOF_MAGIC.to_vec();
        for (ty, v) in &self.records {
            write_bigsize(*ty, &mut out);
            write_bigsize(v.len() as u64, &mut out);
            out.extend_from_slice(v);
        }
        out
    }
}

/// Parse an lnd TLV stream (`BigSize type || BigSize len || value`) with strictly
/// ascending types, consuming all bytes.
fn parse_tlv(data: &[u8]) -> Result<Vec<(u64, &[u8])>, ProofError> {
    let mut c = Cursor { b: data, i: 0 };
    let mut records = Vec::new();
    let mut last: Option<u64> = None;
    while c.i < data.len() {
        let ty = c.bigsize()?;
        if let Some(prev) = last {
            if ty <= prev {
                return Err(ProofError::TlvOutOfOrder);
            }
        }
        last = Some(ty);
        let len = c.bigsize()? as usize;
        records.push((ty, c.take(len)?));
    }
    Ok(records)
}

/// Read a single canonical BigSize that must consume the whole slice.
fn bigsize_exact(v: &[u8]) -> Result<u64, ProofError> {
    let mut c = Cursor { b: v, i: 0 };
    let x = c.bigsize()?;
    if c.i != v.len() {
        return Err(ProofError::TrailingBytes);
    }
    Ok(x)
}

/// Parse a single `proof.Proof` blob (`"TAPP"` + TLV) into its records.
pub fn parse_proof(blob: &[u8]) -> Result<Proof<'_>, ProofError> {
    if blob.get(0..4) != Some(&PROOF_MAGIC[..]) {
        if blob.len() < 4 {
            return Err(ProofError::Truncated);
        }
        return Err(ProofError::BadProofMagic);
    }
    Ok(Proof {
        records: parse_tlv(&blob[4..])?,
    })
}

/// `asset.Asset` TLV type numbers (tapd v0.7.2 `asset/records.go`).
pub mod asset_tlv {
    pub const VERSION: u64 = 0;
    pub const GENESIS: u64 = 2;
    pub const TYPE: u64 = 4;
    pub const AMOUNT: u64 = 6;
    pub const LOCK_TIME: u64 = 7;
    pub const RELATIVE_LOCK_TIME: u64 = 9;
    pub const PREV_WITNESS: u64 = 11;
    pub const SPLIT_COMMITMENT_ROOT: u64 = 13;
    pub const SCRIPT_VERSION: u64 = 14;
    pub const SCRIPT_KEY: u64 = 16;
    pub const GROUP_KEY: u64 = 17;
}

/// A Taproot Asset genesis (`asset.Genesis`): identifies the asset's origin.
#[derive(Debug, PartialEq, Eq)]
pub struct Genesis<'a> {
    pub first_prev_out_txid: [u8; 32],
    pub first_prev_out_vout: u32,
    pub tag: &'a [u8],
    pub meta_hash: [u8; 32],
    pub output_index: u32,
    pub asset_type: u8,
}

impl Genesis<'_> {
    /// `asset_id = sha256( txid || u32_LE(vout) || sha256(tag) || meta_hash ||
    /// u32_BE(output_index) || type )` — `Genesis.ID()` in tapd. NB the prev-out
    /// index is little-endian (Bitcoin wire) while `output_index` is big-endian;
    /// verified byte-for-byte against a live tapd mint.
    pub fn asset_id(&self) -> [u8; 32] {
        let mut h = Sha256::new();
        h.update(self.first_prev_out_txid);
        h.update(self.first_prev_out_vout.to_le_bytes());
        h.update(Sha256::digest(self.tag));
        h.update(self.meta_hash);
        h.update(self.output_index.to_be_bytes());
        h.update([self.asset_type]);
        h.finalize().into()
    }
}

/// The decoded fields of a proof's `ASSET_LEAF` (the `asset.Asset` at this step).
/// `prev_witness` / `split_commitment_root` are kept raw pending later increments.
#[derive(Debug, PartialEq, Eq)]
pub struct AssetLeaf<'a> {
    pub version: u8,
    pub genesis: Genesis<'a>,
    pub asset_type: u8,
    pub amount: u64,
    pub script_version: u16,
    /// 33-byte compressed Taproot Asset script key.
    pub script_key: &'a [u8],
    pub group_key: Option<&'a [u8]>,
}

fn parse_genesis(v: &[u8]) -> Result<Genesis<'_>, ProofError> {
    let mut c = Cursor { b: v, i: 0 };
    let first_prev_out_txid: [u8; 32] = c.take(32)?.try_into().unwrap();
    let first_prev_out_vout = c.u32_be()?;
    let tag_len = c.bigsize()? as usize;
    let tag = c.take(tag_len)?;
    let meta_hash: [u8; 32] = c.take(32)?.try_into().unwrap();
    let output_index = c.u32_be()?;
    let asset_type = c.take(1)?[0];
    if c.i != v.len() {
        return Err(ProofError::TrailingBytes);
    }
    Ok(Genesis {
        first_prev_out_txid,
        first_prev_out_vout,
        tag,
        meta_hash,
        output_index,
        asset_type,
    })
}

/// Decode an `ASSET_LEAF` value (the nested `asset.Asset` TLV stream).
pub fn parse_asset_leaf(value: &[u8]) -> Result<AssetLeaf<'_>, ProofError> {
    use asset_tlv::*;
    let recs = parse_tlv(value)?;
    let get = |t: u64| recs.iter().find(|(ty, _)| *ty == t).map(|(_, v)| *v);
    let req = |t: u64| get(t).ok_or(ProofError::Truncated);
    let u8_at = |v: &[u8]| -> Result<u8, ProofError> {
        <[u8; 1]>::try_from(v)
            .map(|b| b[0])
            .map_err(|_| ProofError::Truncated)
    };
    Ok(AssetLeaf {
        version: u8_at(req(VERSION)?)?,
        genesis: parse_genesis(req(GENESIS)?)?,
        asset_type: u8_at(req(TYPE)?)?,
        amount: bigsize_exact(req(AMOUNT)?)?, // amount is itself a BigSize
        script_version: u16::from_be_bytes(
            <[u8; 2]>::try_from(req(SCRIPT_VERSION)?).map_err(|_| ProofError::Truncated)?,
        ),
        script_key: req(SCRIPT_KEY)?,
        group_key: get(GROUP_KEY),
    })
}

/// `proof.TaprootProof` TLV type numbers (tapd v0.7.2 `proof/records.go`).
pub mod taproot_tlv {
    pub const OUTPUT_INDEX: u64 = 0;
    pub const INTERNAL_KEY: u64 = 2;
    pub const COMMITMENT_PROOF: u64 = 3;
    pub const TAPSCRIPT_PROOF: u64 = 5;
}

/// A `proof.TaprootProof`: how the Taproot Asset commitment is anchored in (or
/// excluded from) one Bitcoin output. The `commitment_proof` (asset → MS-SMT →
/// taproot output key) is kept raw pending the MS-SMT increment.
#[derive(Debug, PartialEq, Eq)]
pub struct TaprootProof<'a> {
    pub output_index: u32,
    pub internal_key: &'a [u8],
    pub commitment_proof: Option<&'a [u8]>,
    pub tapscript_proof: Option<&'a [u8]>,
}

/// `commitment.Proof` (CommitmentProof) TLV types — the asset-inclusion side of a
/// TaprootProof (tapd v0.7.2 `commitment/proof.go`).
pub mod commitment_tlv {
    pub const ASSET_PROOF: u64 = 1;
    pub const TAPROOT_ASSET_PROOF: u64 = 2;
    /// Optional tapscript sibling preimage hashed with the TA-commitment leaf to
    /// form the output's tapscript root (SatUSD lock anchors carry one).
    pub const TAP_SIBLING_PREIMAGE: u64 = 5;
    // AssetProof inner TLV
    pub const AP_VERSION: u64 = 0;
    pub const AP_ASSET_ID: u64 = 2; // the AssetCommitment's tap key (asset_id)
    pub const AP_MSSMT_PROOF: u64 = 4;
    // TaprootAssetProof inner TLV
    pub const TAP_VERSION: u64 = 0;
    pub const TAP_MSSMT_PROOF: u64 = 2;
}

/// Proof that the asset is in its `AssetCommitment` (inner MS-SMT).
#[derive(Debug, PartialEq, Eq)]
pub struct AssetProof<'a> {
    pub version: u8,
    /// The AssetCommitment's key (the asset_id).
    pub asset_id: &'a [u8],
    /// Raw compressed MS-SMT inclusion proof (decoded in the MS-SMT increment).
    pub mssmt_proof: &'a [u8],
}

/// Proof that the `AssetCommitment` is in the `TapCommitment` (outer MS-SMT).
#[derive(Debug, PartialEq, Eq)]
pub struct TaprootAssetProof<'a> {
    /// TapCommitment version (0/1/2).
    pub version: u8,
    pub mssmt_proof: &'a [u8],
}

/// A `commitment.Proof`: the two-level (asset → AssetCommitment → TapCommitment)
/// inclusion proof carried in a `TaprootProof.commitment_proof`.
#[derive(Debug, PartialEq, Eq)]
pub struct CommitmentProof<'a> {
    pub asset_proof: AssetProof<'a>,
    pub taproot_asset_proof: TaprootAssetProof<'a>,
    /// Optional tapscript sibling preimage (`[siblingType:u8] || preimage`): a
    /// leaf (`version || compactsize(len) || script`) or a 64-byte branch
    /// (`left || right`). When present the output's tapscript root is the
    /// `TapBranch` of the TA-commitment leaf with this sibling.
    pub tap_sibling: Option<&'a [u8]>,
}

fn u8_field(v: &[u8]) -> Result<u8, ProofError> {
    <[u8; 1]>::try_from(v)
        .map(|b| b[0])
        .map_err(|_| ProofError::BadFieldLength)
}

/// Parse a `TaprootProof.commitment_proof` value into its two-level structure.
pub fn parse_commitment_proof(value: &[u8]) -> Result<CommitmentProof<'_>, ProofError> {
    use commitment_tlv::*;
    let recs = parse_tlv(value)?;
    let get = |t: u64| recs.iter().find(|(ty, _)| *ty == t).map(|(_, v)| *v);

    let ap = parse_tlv(get(ASSET_PROOF).ok_or(ProofError::Truncated)?)?;
    let apg = |t: u64| ap.iter().find(|(ty, _)| *ty == t).map(|(_, v)| *v);
    let asset_proof = AssetProof {
        version: u8_field(apg(AP_VERSION).ok_or(ProofError::Truncated)?)?,
        asset_id: apg(AP_ASSET_ID).ok_or(ProofError::Truncated)?,
        mssmt_proof: apg(AP_MSSMT_PROOF).ok_or(ProofError::Truncated)?,
    };

    let tp = parse_tlv(get(TAPROOT_ASSET_PROOF).ok_or(ProofError::Truncated)?)?;
    let tpg = |t: u64| tp.iter().find(|(ty, _)| *ty == t).map(|(_, v)| *v);
    let taproot_asset_proof = TaprootAssetProof {
        version: u8_field(tpg(TAP_VERSION).ok_or(ProofError::Truncated)?)?,
        mssmt_proof: tpg(TAP_MSSMT_PROOF).ok_or(ProofError::Truncated)?,
    };

    Ok(CommitmentProof {
        asset_proof,
        taproot_asset_proof,
        tap_sibling: get(TAP_SIBLING_PREIMAGE),
    })
}

impl<'a> TaprootProof<'a> {
    /// Decode the asset-inclusion `commitment_proof`, if present.
    pub fn parse_commitment(&self) -> Option<Result<CommitmentProof<'a>, ProofError>> {
        self.commitment_proof.map(parse_commitment_proof)
    }
}

/// Parse an `INCLUSION_PROOF`/`EXCLUSION_PROOFS` value (a `TaprootProof` TLV).
pub fn parse_taproot_proof(value: &[u8]) -> Result<TaprootProof<'_>, ProofError> {
    use taproot_tlv::*;
    let recs = parse_tlv(value)?;
    let get = |t: u64| recs.iter().find(|(ty, _)| *ty == t).map(|(_, v)| *v);
    let output_index = u32::from_be_bytes(
        <[u8; 4]>::try_from(get(OUTPUT_INDEX).ok_or(ProofError::Truncated)?)
            .map_err(|_| ProofError::BadFieldLength)?,
    );
    Ok(TaprootProof {
        output_index,
        internal_key: get(INTERNAL_KEY).ok_or(ProofError::Truncated)?,
        commitment_proof: get(COMMITMENT_PROOF),
        tapscript_proof: get(TAPSCRIPT_PROOF),
    })
}

/// Witness sub-record TLV types (tapd `asset/records.go`).
mod witness_tlv {
    pub const PREV_ID: u64 = 1;
    pub const TX_WITNESS: u64 = 3;
    pub const SPLIT_COMMITMENT: u64 = 5;
}

/// The asset-leaf bytes that tapd commits to its output. For a split output —
/// an asset whose single prev-witness is a split-commitment witness (PrevID
/// present, no TxWitness, SplitCommitment present) — tapd nulls the
/// `SplitCommitment` before hashing the leaf (proof/taproot.go
/// `DeriveByAssetInclusion`: "the output of the receiver was created without
/// this present"). This re-encodes the asset leaf with that sub-record stripped.
/// Returns `None` when the asset is not a split-commitment witness, in which case
/// the raw asset-leaf bytes are already the committed leaf (e.g. mint outputs).
fn split_committed_leaf_bytes(asset_leaf_bytes: &[u8]) -> Option<Vec<u8>> {
    let recs = parse_tlv(asset_leaf_bytes).ok()?;
    let pw = recs
        .iter()
        .find(|(t, _)| *t == asset_tlv::PREV_WITNESS)
        .map(|(_, v)| *v)?;

    // PrevWitness value = BigSize(count) || count×( BigSize(len) || witness_tlv ).
    // A split-commitment witness asset has exactly one prev-witness.
    let mut c = Cursor { b: pw, i: 0 };
    if c.bigsize().ok()? != 1 {
        return None;
    }
    let wlen = c.bigsize().ok()? as usize;
    let witness = c.take(wlen).ok()?;
    if c.i != pw.len() {
        return None;
    }

    // IsSplitCommitWitness: PrevID present, no TxWitness, SplitCommitment present.
    let wrecs = parse_tlv(witness).ok()?;
    let has = |t: u64| wrecs.iter().any(|(ty, _)| *ty == t);
    if !(has(witness_tlv::PREV_ID)
        && !has(witness_tlv::TX_WITNESS)
        && has(witness_tlv::SPLIT_COMMITMENT))
    {
        return None;
    }

    // Re-encode the witness without the split-commitment (type 5) record, then
    // re-wrap the PrevWitness value, then re-encode the whole asset TLV stream.
    let mut new_witness = Vec::new();
    for (ty, v) in &wrecs {
        if *ty == witness_tlv::SPLIT_COMMITMENT {
            continue;
        }
        write_bigsize(*ty, &mut new_witness);
        write_bigsize(v.len() as u64, &mut new_witness);
        new_witness.extend_from_slice(v);
    }
    let mut new_pw = Vec::new();
    write_bigsize(1, &mut new_pw);
    write_bigsize(new_witness.len() as u64, &mut new_pw);
    new_pw.extend_from_slice(&new_witness);

    let mut out = Vec::new();
    for (ty, v) in &recs {
        write_bigsize(*ty, &mut out);
        let value: &[u8] = if *ty == asset_tlv::PREV_WITNESS {
            &new_pw
        } else {
            v
        };
        write_bigsize(value.len() as u64, &mut out);
        out.extend_from_slice(value);
    }
    Some(out)
}

fn dsha256(parts: &[&[u8]]) -> [u8; 32] {
    let mut h = Sha256::new();
    for p in parts {
        h.update(p);
    }
    let first = h.finalize();
    Sha256::digest(first).into()
}

/// A `proof.TxMerkleProof`: sibling `nodes` + packed direction `bits` proving the
/// anchor tx is in the block's merkle root.
pub struct TxMerkleProof<'a> {
    pub nodes: Vec<&'a [u8]>,
    bits_packed: &'a [u8],
}

impl TxMerkleProof<'_> {
    /// bit `i`: 0 ⇒ current is the right child (node on the left), 1 ⇒ current is
    /// the left child. Packed LSB-first (tapd `packBits` sets `1 << (i%8)`),
    /// zero-extended. (Real multi-node signet/mainnet proofs exercise this;
    /// trivial regtest proofs have all-zero bits, which hid the order.)
    fn bit(&self, i: usize) -> bool {
        let byte = self.bits_packed.get(i / 8).copied().unwrap_or(0);
        (byte >> (i % 8)) & 1 == 1
    }

    /// Fold `txid` (internal byte order) up through the nodes to a merkle root.
    pub fn root(&self, txid: [u8; 32]) -> [u8; 32] {
        let mut cur = txid;
        for (i, node) in self.nodes.iter().enumerate() {
            cur = if self.bit(i) {
                dsha256(&[&cur, node]) // current is left
            } else {
                dsha256(&[node, &cur]) // current is right
            };
        }
        cur
    }
}

/// Parse a `TX_MERKLE_PROOF` value: `BigSize(numNodes) || nodes×32 ||
/// ceil(numNodes/8) packed direction-bit bytes`. The bits are RAW (LSB-first),
/// NOT length-prefixed — a single 0x00 byte makes a length-prefixed misread
/// look correct, which is why trivial regtest proofs hid this.
pub fn parse_tx_merkle_proof(value: &[u8]) -> Result<TxMerkleProof<'_>, ProofError> {
    let mut c = Cursor { b: value, i: 0 };
    let n = c.bigsize()? as usize;
    let mut nodes = Vec::with_capacity(n);
    for _ in 0..n {
        nodes.push(c.take(32)?);
    }
    let bits_packed = c.take(n.div_ceil(8))?;
    if c.i != value.len() {
        return Err(ProofError::TrailingBytes);
    }
    Ok(TxMerkleProof { nodes, bits_packed })
}

/// The verified on-chain anchoring of a proof step.
#[derive(Debug, PartialEq, Eq)]
pub struct AnchorInfo {
    /// Anchor tx id (internal byte order).
    pub txid: [u8; 32],
    pub output_index: u32,
    /// The anchored P2TR output's 32-byte x-only taproot output key.
    pub taproot_output_key: [u8; 32],
}

/// Verify a proof step's **on-chain anchoring** (incr 4a): the anchor tx is
/// committed in the block header's merkle root (via the proof's TxMerkleProof),
/// and the TaprootProof's output index points at a P2TR output. Returns the
/// anchored taproot output key — the target the MS-SMT commitment reconstruction
/// (incr 4b) must reproduce from the asset. Does NOT yet verify the asset→key
/// commitment, nor that the header is on the best chain (that is the §5.D14 SPV
/// layer / ClaimClock).
pub fn verify_anchor(proof: &Proof) -> Result<AnchorInfo, ProofError> {
    let header = proof.get(tlv::BLOCK_HEADER).ok_or(ProofError::Truncated)?;
    if header.len() != 80 {
        return Err(ProofError::BadFieldLength);
    }
    let merkle_root: [u8; 32] = header[36..68].try_into().unwrap();

    let tx_bytes = proof.get(tlv::ANCHOR_TX).ok_or(ProofError::Truncated)?;
    let tx: bitcoin::Transaction =
        bitcoin::consensus::deserialize(tx_bytes).map_err(|_| ProofError::BadAnchorTx)?;
    use bitcoin::hashes::Hash as _;
    let txid: [u8; 32] = tx.compute_txid().to_raw_hash().to_byte_array();

    let mp = parse_tx_merkle_proof(
        proof
            .get(tlv::TX_MERKLE_PROOF)
            .ok_or(ProofError::Truncated)?,
    )?;
    if mp.root(txid) != merkle_root {
        return Err(ProofError::AnchorMerkleMismatch);
    }

    let tp = parse_taproot_proof(
        proof
            .get(tlv::INCLUSION_PROOF)
            .ok_or(ProofError::Truncated)?,
    )?;
    let out = tx
        .output
        .get(tp.output_index as usize)
        .ok_or(ProofError::OutputIndexOutOfRange)?;
    if !out.script_pubkey.is_p2tr() {
        return Err(ProofError::NotP2tr);
    }
    // P2TR scriptPubKey = OP_1 (0x51) PUSH32 <x-only output key>.
    let spk = out.script_pubkey.as_bytes();
    if spk.len() != 34 {
        return Err(ProofError::BadFieldLength);
    }
    let taproot_output_key: [u8; 32] = spk[2..34].try_into().unwrap();

    Ok(AnchorInfo {
        txid,
        output_index: tp.output_index,
        taproot_output_key,
    })
}

fn sha256(parts: &[&[u8]]) -> [u8; 32] {
    let mut h = Sha256::new();
    for p in parts {
        h.update(p);
    }
    h.finalize().into()
}

/// BIP341 tagged hash: `sha256(sha256(tag) || sha256(tag) || msg)`.
fn tagged_hash(tag: &[u8], msg: &[u8]) -> [u8; 32] {
    let th = Sha256::digest(tag);
    sha256(&[&th, &th, msg])
}

/// BIP341 `TapBranch`: hash the two child hashes in lexicographic order
/// (`tapd asset.TapBranchHash`).
fn tap_branch_hash(a: &[u8; 32], b: &[u8; 32]) -> [u8; 32] {
    let (l, r) = if a[..] <= b[..] { (a, b) } else { (b, a) };
    let mut msg = [0u8; 64];
    msg[..32].copy_from_slice(l);
    msg[32..].copy_from_slice(r);
    tagged_hash(b"TapBranch", &msg)
}

/// Tap-hash of a tapd `TapscriptPreimage` wire value (`[siblingType:u8] ||
/// preimage`). A leaf preimage (type 0) is already a TapLeaf preimage
/// (`leafVersion || compactsize(len) || script`) ⇒ `TapLeaf` tagged hash; a
/// branch preimage (type 1) is `left(32) || right(32)` ⇒ `TapBranch`.
fn sibling_tap_hash(value: &[u8]) -> Result<[u8; 32], ProofError> {
    let (ty, preimage) = value.split_first().ok_or(ProofError::BadTapSibling)?;
    match ty {
        0 => Ok(tagged_hash(b"TapLeaf", preimage)),
        1 => {
            if preimage.len() != 64 {
                return Err(ProofError::BadTapSibling);
            }
            let l: [u8; 32] = preimage[..32].try_into().unwrap();
            let r: [u8; 32] = preimage[32..].try_into().unwrap();
            Ok(tap_branch_hash(&l, &r))
        }
        _ => Err(ProofError::BadTapSibling),
    }
}

/// Reconstruct the on-chain x-only taproot output key that a proof's asset must
/// be committed under, from the asset leaf + its `CommitmentProof`. This is the
/// cryptographic heart of DL-23: it ties the asset (amount, script_key) to the
/// Bitcoin output via the asset→AssetCommitment→TapCommitment MS-SMTs, the V2
/// tapscript leaf, and the taproot tweak. Verified byte-for-byte against live
/// tapd (`commitment`/`mssmt`).
///
/// Handles mint and transfer (split) outputs, general compressed MS-SMT proofs,
/// and an optional tapscript sibling (lock anchors). Scope still pending:
/// multi-asset commitments, V0/V1 TapCommitments, grouped assets.
pub fn reconstruct_output_key(
    asset: &AssetLeaf,
    asset_leaf_bytes: &[u8],
    cp: &CommitmentProof,
    taproot: &TaprootProof,
) -> Result<[u8; 32], ProofError> {
    if taproot.internal_key.len() != 33 {
        return Err(ProofError::BadFieldLength);
    }
    let asset_id: [u8; 32] = cp
        .asset_proof
        .asset_id
        .try_into()
        .map_err(|_| ProofError::BadFieldLength)?;

    // Inner MS-SMT: asset in its AssetCommitment, keyed by sha256(xonly(scriptkey)).
    // Fold through the proof's own siblings (single-leaf for mint outputs, but
    // transfer outputs commit additional leaves — e.g. STXO — so the siblings
    // are not all empty: `walkUp`, not an assumed empty path).
    if asset.script_key.len() != 33 {
        return Err(ProofError::BadFieldLength);
    }
    // Inner AssetCommitment key (`asset.AssetCommitmentKey`): for a grouped asset
    // (issuance enabled) it is `sha256(genesis_asset_id || xonly(scriptkey))`;
    // for an ungrouped asset just `sha256(xonly(scriptkey))`.
    let ack = if asset.group_key.is_some() {
        sha256(&[&asset.genesis.asset_id(), &asset.script_key[1..33]])
    } else {
        sha256(&[&asset.script_key[1..33]])
    };
    // Split outputs commit the asset leaf with the split-commitment witness
    // stripped; mint outputs commit the raw leaf.
    let stripped = split_committed_leaf_bytes(asset_leaf_bytes);
    let leaf_value: &[u8] = stripped.as_deref().unwrap_or(asset_leaf_bytes);
    let inner_sib = mssmt::parse_compressed_proof(cp.asset_proof.mssmt_proof)
        .ok_or(ProofError::BadFieldLength)?;
    let (inner, l, r) = mssmt::proof_root(&ack, mssmt::leaf(leaf_value, asset.amount), &inner_sib);

    // AssetCommitment.Root() = sha256(asset_id || rootL || rootR || u64_be(sum)).
    let ac_root = sha256(&[&asset_id, &l, &r, &inner.sum.to_be_bytes()]);

    // Outer MS-SMT: AssetCommitment in the TapCommitment, keyed by asset_id.
    // Leaf value = [ac_version] || ac.Root() || u64_be(sum).
    let mut tcl_value = Vec::with_capacity(41);
    tcl_value.push(cp.asset_proof.version);
    tcl_value.extend_from_slice(&ac_root);
    tcl_value.extend_from_slice(&inner.sum.to_be_bytes());
    let outer_sib = mssmt::parse_compressed_proof(cp.taproot_asset_proof.mssmt_proof)
        .ok_or(ProofError::BadFieldLength)?;
    let (outer, _, _) =
        mssmt::proof_root(&asset_id, mssmt::leaf(&tcl_value, inner.sum), &outer_sib);

    // V2 tapscript leaf: tag || version || rootHash || u64_be(rootSum).
    if cp.taproot_asset_proof.version != 2 {
        return Err(ProofError::UnsupportedTapVersion);
    }
    let tag = sha256(&[b"taproot-assets:194243"]);
    let mut script = Vec::with_capacity(73);
    script.extend_from_slice(&tag);
    script.push(2);
    script.extend_from_slice(&outer.hash);
    script.extend_from_slice(&outer.sum.to_be_bytes());
    // TapLeaf hash (leaf version 0xc0, BIP341); script len 73 < 0xfd.
    let mut preimage = Vec::with_capacity(2 + script.len());
    preimage.push(0xc0);
    preimage.push(script.len() as u8);
    preimage.extend_from_slice(&script);
    let ta_leaf_hash = tagged_hash(b"TapLeaf", &preimage);

    // Tapscript root: the TA-commitment leaf alone, or — if the output committed
    // a tapscript sibling (e.g. a SatUSD lock's finalize/refund tree) — the
    // BIP341 `TapBranch` of the TA leaf with the sibling's tap hash.
    let tapscript_root = match cp.tap_sibling {
        None => ta_leaf_hash,
        Some(sib) => tap_branch_hash(&ta_leaf_hash, &sibling_tap_hash(sib)?),
    };

    // Taproot output key = taptweak(internal_xonly, tapscript_root).
    let internal_xonly: [u8; 32] = taproot.internal_key[1..33]
        .try_into()
        .map_err(|_| ProofError::BadFieldLength)?;
    Ok(satusd_crypto::nums::tap_tweak(
        &internal_xonly,
        &tapscript_root,
    ))
}

/// Verify a proof step is genuinely anchored AND that its asset is committed in
/// that anchor output (DL-23 inclusion): the reconstructed taproot output key
/// must equal the on-chain anchored output key. Returns the verified `AnchorInfo`.
pub fn verify_asset_inclusion(proof: &Proof) -> Result<AnchorInfo, ProofError> {
    let anchor = verify_anchor(proof)?;
    let asset = proof.asset_leaf()?;
    let asset_leaf_bytes = proof.get(tlv::ASSET_LEAF).ok_or(ProofError::Truncated)?;
    let taproot = parse_taproot_proof(
        proof
            .get(tlv::INCLUSION_PROOF)
            .ok_or(ProofError::Truncated)?,
    )?;
    let cp = taproot.parse_commitment().ok_or(ProofError::Truncated)??;
    let key = reconstruct_output_key(&asset, asset_leaf_bytes, &cp, &taproot)?;
    if key != anchor.taproot_output_key {
        return Err(ProofError::CommitmentMismatch);
    }
    Ok(anchor)
}

/// A proof step's input outpoint (`PREV_OUT`, type 2): txid (internal order) +
/// vout (u32 big-endian, matching tapd's OutPoint encoding).
fn prev_outpoint(proof: &Proof) -> Result<([u8; 32], u32), ProofError> {
    let v = proof.get(tlv::PREV_OUT).ok_or(ProofError::Truncated)?;
    if v.len() != 36 {
        return Err(ProofError::BadFieldLength);
    }
    let txid: [u8; 32] = v[0..32].try_into().unwrap();
    let vout = u32::from_be_bytes(v[32..36].try_into().unwrap());
    Ok((txid, vout))
}

/// Verify a proof file's lineage: every step's asset is committed in its anchor
/// output (`verify_asset_inclusion`) AND each step's input (`prev_out`) is the
/// previous step's anchored output — a connected chain of Bitcoin transactions,
/// each cryptographically committing its asset, from genesis to the latest state.
/// Returns the head (latest) `AnchorInfo`.
///
/// Both mint outputs and transfer (split) outputs are fully verified: split
/// outputs commit the asset leaf with the split-commitment witness stripped
/// (handled in `reconstruct_output_key`).
pub fn verify_lineage(proofs: &[Proof]) -> Result<AnchorInfo, ProofError> {
    let mut prev: Option<AnchorInfo> = None;
    for p in proofs {
        let anchor = verify_asset_inclusion(p)?;
        if let Some(prev) = &prev {
            let (txid, vout) = prev_outpoint(p)?;
            if txid != prev.txid || vout != prev.output_index {
                return Err(ProofError::LineageBroken);
            }
        }
        prev = Some(anchor);
    }
    prev.ok_or(ProofError::EmptyLineage)
}

/// Encode a value as an lnd BigSize varint (big-endian, canonical).
fn write_bigsize(v: u64, out: &mut Vec<u8>) {
    if v < 0xfd {
        out.push(v as u8);
    } else if v <= 0xffff {
        out.push(0xfd);
        out.extend_from_slice(&(v as u16).to_be_bytes());
    } else if v <= 0xffff_ffff {
        out.push(0xfe);
        out.extend_from_slice(&(v as u32).to_be_bytes());
    } else {
        out.push(0xff);
        out.extend_from_slice(&v.to_be_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Real proofs exported from a live tapd v0.7.2 (ExportProof) on the SatUSD
    // regtest devnet — see integration/lineage_vectors/.
    const GENESIS_HEX: &str = include_str!("../../../integration/lineage_vectors/genesis.hex");
    const TRANSFER_HEX: &str = include_str!("../../../integration/lineage_vectors/transfer.hex");
    // A real *signet* canonical-SatUSD (grouped, d0c0fb17…) 3-hop lineage. Unlike
    // the regtest vectors its blocks hold many txs, so each TxMerkleProof carries
    // multiple sibling nodes with non-zero LSB-first direction bits — the case
    // that exposes a length-prefixed/MSB-first merkle-proof misparse.
    const SIGNET_HEX: &str = include_str!("../../../integration/lineage_vectors/signet_transfer.hex");

    fn bytes(hex_str: &str) -> Vec<u8> {
        hex::decode(hex_str.trim()).unwrap()
    }

    fn h32(hex_str: &str) -> [u8; 32] {
        hex::decode(hex_str).unwrap().try_into().unwrap()
    }

    #[test]
    fn signet_multinode_merkle_proof_verifies() {
        // Regression for the TxMerkleProof bits parse + order. Real signet blocks
        // hold many txs, so each proof's merkle path has several sibling nodes
        // with non-zero LSB-first direction bits — a length-prefixed / MSB-first
        // read gets it wrong (all-zero regtest bits hid it).
        let data = bytes(SIGNET_HEX);
        let f = parse_proof_file(&data).expect("parse signet proof file");
        let proofs = f.parsed().expect("parse proofs");
        assert!(proofs.len() >= 2, "expected a multi-hop lineage");

        // The fixture must actually contain a multi-node merkle proof, else this
        // test would pass even with the old (trivial-only) behaviour.
        let any_multinode = proofs.iter().any(|p| {
            p.get(tlv::TX_MERKLE_PROOF)
                .and_then(|v| parse_tx_merkle_proof(v).ok())
                .is_some_and(|mp| mp.nodes.len() > 1)
        });
        assert!(any_multinode, "fixture should contain a multi-node merkle proof");

        // Each step's anchor tx must reconstruct its block header merkle root
        // (directly exercises the bits fix), then the full DL-23 lineage holds.
        for (i, p) in proofs.iter().enumerate() {
            verify_anchor(p).unwrap_or_else(|e| panic!("anchor {i} failed: {e:?}"));
        }
        verify_lineage(&proofs).expect("signet canonical-SatUSD lineage verifies");
    }

    #[test]
    fn genesis_is_single_proof() {
        let data = bytes(GENESIS_HEX);
        let f = parse_proof_file(&data).expect("parse genesis");
        assert_eq!(f.version, 0);
        assert_eq!(f.proofs.len(), 1);
        assert_eq!(&f.proofs[0][0..4], &PROOF_MAGIC[..]);
    }

    #[test]
    fn transfer_is_two_proof_lineage() {
        // genesis → one transfer ⇒ a 2-proof hash-chained lineage.
        let data = bytes(TRANSFER_HEX);
        let f = parse_proof_file(&data).expect("parse transfer");
        assert_eq!(f.version, 0);
        assert_eq!(f.proofs.len(), 2);
        for p in &f.proofs {
            assert_eq!(&p[0..4], &PROOF_MAGIC[..]);
        }
    }

    #[test]
    fn corrupted_proof_byte_breaks_hash_chain() {
        let mut data = bytes(GENESIS_HEX);
        // Flip a byte inside the proof body (after TAPF+ver+count+len = offset ~11).
        let i = data.len() / 2;
        data[i] ^= 0x01;
        assert_eq!(parse_proof_file(&data), Err(ProofError::ChecksumMismatch));
    }

    #[test]
    fn bad_file_magic_rejected() {
        let mut data = bytes(GENESIS_HEX);
        data[0] ^= 0xff;
        assert_eq!(parse_proof_file(&data), Err(ProofError::BadFileMagic));
    }

    #[test]
    fn truncated_rejected() {
        let data = bytes(GENESIS_HEX);
        assert_eq!(
            parse_proof_file(&data[..data.len() - 10]),
            Err(ProofError::Truncated)
        );
    }

    // ---- inner Proof TLV (increment 2) ----

    #[test]
    fn genesis_proof_tlv_decodes() {
        let data = bytes(GENESIS_HEX);
        let f = parse_proof_file(&data).unwrap();
        let p = parse_proof(f.proofs[0]).expect("parse proof");
        // Exact record set of a genesis (mint) proof — no split_root (15).
        let types: Vec<u64> = p.records.iter().map(|(t, _)| *t).collect();
        assert_eq!(types, vec![0, 2, 4, 6, 8, 10, 12, 13, 17, 22, 23]);
        assert_eq!(p.version(), Some(0));
        assert_eq!(p.block_height(), Some(150)); // ground truth from tapd DecodeProof
        assert!(p.get(tlv::ASSET_LEAF).is_some());
        assert!(p.get(tlv::INCLUSION_PROOF).is_some());
        assert!(p.get(tlv::GENESIS_REVEAL).is_some());
        // Lossless parse: re-encoding reproduces the original proof bytes.
        assert_eq!(p.reencode(), f.proofs[0]);
    }

    #[test]
    fn transfer_lineage_proofs_decode() {
        let data = bytes(TRANSFER_HEX);
        let f = parse_proof_file(&data).unwrap();
        let proofs = f.parsed().expect("parse all");
        assert_eq!(proofs.len(), 2);
        for (p, raw) in proofs.iter().zip(&f.proofs) {
            // Every proof carries the asset leaf + its anchor inclusion proof.
            assert!(p.get(tlv::ASSET_LEAF).is_some());
            assert!(p.get(tlv::INCLUSION_PROOF).is_some());
            assert_eq!(p.reencode().as_slice(), *raw); // lossless round-trip
        }
        // The transfer step is anchored at a height ≥ the genesis step.
        assert!(proofs[1].block_height() >= proofs[0].block_height());
    }

    #[test]
    fn proof_bad_magic_rejected() {
        let data = bytes(GENESIS_HEX);
        let f = parse_proof_file(&data).unwrap();
        let mut p = f.proofs[0].to_vec();
        p[0] ^= 0xff;
        assert_eq!(parse_proof(&p), Err(ProofError::BadProofMagic));
    }

    // ---- asset leaf decode (increment 3) ----

    #[test]
    fn genesis_asset_leaf_decodes() {
        let data = bytes(GENESIS_HEX);
        let f = parse_proof_file(&data).unwrap();
        let p = parse_proof(f.proofs[0]).unwrap();
        let a = p.asset_leaf().expect("asset leaf");

        // Ground truth from the live-tapd mint (DecodeProof / assets list).
        assert_eq!(a.amount, 1_000_000); // BigSize-encoded inside the TLV value
        assert_eq!(a.version, 0);
        assert_eq!(a.asset_type, 0); // NORMAL
        assert_eq!(a.script_version, 0);
        assert_eq!(
            hex::encode(a.script_key),
            "0221c90455bdf31f6e155184dc2f3748f6ee4ad88454075e90c8b324ede1c1f2da"
        );
        assert_eq!(a.group_key, None); // minted without a group key
        assert_eq!(a.genesis.tag, b"satusd_lineage_test");
        assert_eq!(a.genesis.output_index, 0);
        // The derived asset_id must match tapd's.
        assert_eq!(
            hex::encode(a.genesis.asset_id()),
            "ece4593a38bb4f57adc60e1b82395e81026a9eb5be4653d8a783feece52f6e6a"
        );
    }

    // ---- on-chain anchoring (increment 4a) ----

    #[test]
    fn genesis_anchor_verifies() {
        let data = bytes(GENESIS_HEX);
        let f = parse_proof_file(&data).unwrap();
        let p = parse_proof(f.proofs[0]).unwrap();
        let a = verify_anchor(&p).expect("anchor verifies");
        assert_eq!(a.output_index, 0);
        // x-only taproot output key of the mint anchor output (from bitcoind).
        assert_eq!(
            hex::encode(a.taproot_output_key),
            "1698ed3b0b2b7b7447ae96c22dff9f90884094502bd1f31602c250aa3dc6ca88"
        );
        // txid (internal order) == reverse of the display txid.
        let mut disp =
            hex::decode("333c8ef767d8bef43d330f5d4d4df0c0ad2bbe7cd867feee0cc286c6df011ab9")
                .unwrap();
        disp.reverse();
        assert_eq!(a.txid.to_vec(), disp);
    }

    #[test]
    fn lineage_anchors_all_verify() {
        // Every step of the 2-proof lineage is committed in its block.
        let data = bytes(TRANSFER_HEX);
        let f = parse_proof_file(&data).unwrap();
        for p in f.parsed().unwrap() {
            verify_anchor(&p).expect("each lineage step is anchored on-chain");
        }
    }

    #[test]
    fn tampered_anchor_tx_breaks_merkle() {
        // Flip the anchor tx's version byte ⇒ txid changes ⇒ the tx-merkle proof
        // no longer reconstructs the block header's merkle root.
        let data = bytes(GENESIS_HEX);
        let f = parse_proof_file(&data).unwrap();
        let blob = f.proofs[0];
        let tx = parse_proof(blob).unwrap().get(tlv::ANCHOR_TX).unwrap();
        let off = tx.as_ptr() as usize - blob.as_ptr() as usize; // tx start within blob
        let mut copy = blob.to_vec();
        copy[off] ^= 0x01; // version low byte (non-witness ⇒ affects txid)
        let p = parse_proof(&copy).unwrap();
        assert_eq!(verify_anchor(&p), Err(ProofError::AnchorMerkleMismatch));
    }

    // ---- commitment proof decode (increment 4b-i) ----

    // ---- asset→output-key commitment (increment 4b-iii, the heart) ----

    #[test]
    fn genesis_asset_committed_in_anchor() {
        // The reconstructed taproot output key (asset → MS-SMTs → tapleaf →
        // taptweak) equals the on-chain anchored output key.
        let data = bytes(GENESIS_HEX);
        let f = parse_proof_file(&data).unwrap();
        let p = parse_proof(f.proofs[0]).unwrap();
        let anchor = verify_asset_inclusion(&p).expect("asset committed in anchor");
        assert_eq!(
            hex::encode(anchor.taproot_output_key),
            "1698ed3b0b2b7b7447ae96c22dff9f90884094502bd1f31602c250aa3dc6ca88"
        );
    }

    #[test]
    fn lineage_continuity_holds() {
        // genesis → transfer: each step's asset is committed in its anchor, and
        // the transfer's input is the genesis's anchored output. Head = transfer.
        let data = bytes(TRANSFER_HEX);
        let f = parse_proof_file(&data).unwrap();
        let proofs = f.parsed().unwrap();
        let head = verify_lineage(&proofs).expect("connected, fully-committed lineage");
        // The head's prev_out chains back to the genesis mint output.
        let (txid, vout) = prev_outpoint(&proofs[1]).unwrap();
        let genesis_anchor = verify_anchor(&proofs[0]).unwrap();
        assert_eq!(txid, genesis_anchor.txid);
        assert_eq!(vout, genesis_anchor.output_index);
        assert_eq!(head.output_index, 1); // the 250000 split output (vout 0 = change)
    }

    // ---- tapscript siblings (lock anchors) ----
    // Ground truth from the Go oracle (tapd `commitment`/`txscript`) over the
    // split asset: ta_leaf_hash af8eb0fc…, a leaf sibling (OP_TRUE) and a branch
    // sibling, with their tap hashes and the resulting `TapscriptRoot(sibling)`.

    #[test]
    fn sibling_tap_hash_leaf_and_branch() {
        // Leaf preimage wire `00 c0 01 51` ⇒ TapLeaf(leafVersion 0xc0, [OP_TRUE]).
        assert_eq!(
            sibling_tap_hash(&hex::decode("00c00151").unwrap()).unwrap(),
            h32("a85b2107f791b26a84e7586c28cec7cb61202ed3d01944d832500f363782d675")
        );
        // Branch preimage wire `01 || left(32) || right(32)`.
        let branch = "01a85b2107f791b26a84e7586c28cec7cb61202ed3d01944d832500f363782d675\
                      c276fef1386890619b80e10a4a328572d97493add269df1a15a7f89f8ae8ec09";
        assert_eq!(
            sibling_tap_hash(&hex::decode(branch).unwrap()).unwrap(),
            h32("6496f0779f38b871013be71ee7dcce8fcdcc02afc4c688acb159fc5de2fba55e")
        );
    }

    #[test]
    fn tap_branch_hash_matches_tapd() {
        // TapBranch(ta_leaf, sibling) == tapd's TapscriptRoot(sibling), for both
        // a leaf sibling and a branch sibling (BIP341 lexicographic sort).
        let ta_leaf = h32("af8eb0fc01e5e6d342b48deec552452c6fff2c61ac21ce229f97aca5468eae7f");
        let leaf_sib = h32("a85b2107f791b26a84e7586c28cec7cb61202ed3d01944d832500f363782d675");
        assert_eq!(
            tap_branch_hash(&ta_leaf, &leaf_sib),
            h32("8d8826d1ed3d11cdf23d1be46fed96ad2bb26129c627bc96d5c9f6383af47225")
        );
        let branch_sib = h32("6496f0779f38b871013be71ee7dcce8fcdcc02afc4c688acb159fc5de2fba55e");
        assert_eq!(
            tap_branch_hash(&ta_leaf, &branch_sib),
            h32("aff8777b1a315b81813cdd802491d8975b93c19032a5bf8abf20154d8eb48aa3")
        );
    }

    #[test]
    fn bad_sibling_rejected() {
        assert_eq!(sibling_tap_hash(&[]), Err(ProofError::BadTapSibling));
        assert_eq!(sibling_tap_hash(&[2, 0, 0]), Err(ProofError::BadTapSibling)); // unknown type
        assert_eq!(sibling_tap_hash(&[1, 0, 0]), Err(ProofError::BadTapSibling));
        // branch ≠ 64B
    }

    #[test]
    fn reconstruct_with_tapscript_sibling() {
        // Inject a leaf tapscript sibling into the split output's commitment proof
        // (as a lock anchor would carry): the reconstructed key must equal
        // taptweak(internal, TapBranch(ta_leaf, sibling)) — tapd's tapscript root.
        let sib_wire = hex::decode("00c00151").unwrap();
        let data = bytes(TRANSFER_HEX);
        let f = parse_proof_file(&data).unwrap();
        let p = parse_proof(f.proofs[1]).unwrap();
        let asset = p.asset_leaf().unwrap();
        let leaf_bytes = p.get(tlv::ASSET_LEAF).unwrap();
        let tp = parse_taproot_proof(p.get(tlv::INCLUSION_PROOF).unwrap()).unwrap();
        let mut cp = tp.parse_commitment().unwrap().unwrap();
        cp.tap_sibling = Some(&sib_wire);

        let key = reconstruct_output_key(&asset, leaf_bytes, &cp, &tp).unwrap();
        let internal_xonly: [u8; 32] = tp.internal_key[1..33].try_into().unwrap();
        let ts_root = h32("8d8826d1ed3d11cdf23d1be46fed96ad2bb26129c627bc96d5c9f6383af47225");
        assert_eq!(
            key,
            satusd_crypto::nums::tap_tweak(&internal_xonly, &ts_root)
        );

        // Without the sibling (no-sibling root) the key differs.
        cp.tap_sibling = None;
        assert_ne!(
            reconstruct_output_key(&asset, leaf_bytes, &cp, &tp).unwrap(),
            key
        );
    }

    // ---- grouped (reissuable) assets ----
    const GROUPED_HEX: &str =
        include_str!("../../../integration/lineage_vectors/grouped_transfer.hex");

    #[test]
    fn grouped_lineage_fully_committed() {
        // A grouped asset keys its inner AssetCommitment by
        // sha256(asset_id || xonly(scriptkey)); the whole genesis→transfer lineage
        // must still verify (anchoring + asset→key commitment per step).
        let data = bytes(GROUPED_HEX);
        let f = parse_proof_file(&data).unwrap();
        let proofs = f.parsed().unwrap();
        assert_eq!(proofs.len(), 2);
        let head = verify_lineage(&proofs).expect("grouped lineage verifies end-to-end");
        let leaf = proofs.last().unwrap().asset_leaf().unwrap();
        assert_eq!(leaf.amount, 400_000); // the split output
        assert_eq!(leaf.group_key.map(|g| g.len()), Some(33)); // carries a group key
                                                               // x-only of the head asset's script key is committed under the anchor key.
        assert_eq!(head.output_index, 1);
    }

    #[test]
    fn burn_lineage_fully_committed() {
        // A real tapd burn: genesis → transfers → burn output (50000 units sent to
        // a provably-unspendable burn script key). The whole lineage, including the
        // burn output's asset→key commitment, must verify.
        let data = bytes(include_str!(
            "../../../integration/lineage_vectors/burn_transfer.hex"
        ));
        let f = parse_proof_file(&data).unwrap();
        let proofs = f.parsed().unwrap();
        let head = verify_lineage(&proofs).expect("burn lineage verifies end-to-end");
        let leaf = proofs.last().unwrap().asset_leaf().unwrap();
        assert_eq!(leaf.amount, 50_000); // the burned amount
        assert_eq!(head.output_index, 0); // burn output anchored at vout 0
    }

    #[test]
    fn burn_to_sink_lineage_verifies() {
        // A grouped SatUSD sent to the protocol NUMS burn sink: the whole lineage
        // (genesis → … → burn output) verifies, and the head output commits the
        // asset to the sink script key the state node will bind against (D16).
        let data = bytes(include_str!(
            "../../../integration/lineage_vectors/burn_to_sink.hex"
        ));
        let f = parse_proof_file(&data).unwrap();
        let proofs = f.parsed().unwrap();
        verify_lineage(&proofs).expect("burn-to-sink lineage verifies end-to-end");
        let leaf = proofs.last().unwrap().asset_leaf().unwrap();
        assert_eq!(leaf.amount, 4_000);
        // The committed x-only script key is the protocol burn sink.
        assert_eq!(
            hex::encode(&leaf.script_key[1..33]),
            "c2945baf61de43d509b9bc70aa20759dfd7902011dba68f8e97eae24451d2131"
        );
    }

    #[test]
    fn lock_anchor_with_real_tapscript_sibling_verifies() {
        // A real SatUSD lock anchor: the asset is committed in a P2TR whose
        // tapscript tree is TapBranch(TA-commitment, finalize/refund sibling). The
        // head's commitment proof carries a real TapSiblingPreimage, and the whole
        // lineage verifies — the sibling path against genuine tapd bytes (not just
        // the oracle-synthesised siblings).
        let data = bytes(include_str!(
            "../../../integration/lineage_vectors/lock_anchor.hex"
        ));
        let f = parse_proof_file(&data).unwrap();
        let proofs = f.parsed().unwrap();
        verify_lineage(&proofs).expect("lock-anchor lineage verifies end-to-end");
        let head = proofs.last().unwrap();
        let tp = parse_taproot_proof(head.get(tlv::INCLUSION_PROOF).unwrap()).unwrap();
        let cp = tp.parse_commitment().unwrap().unwrap();
        assert!(
            cp.tap_sibling.is_some(),
            "lock anchor commits a tapscript sibling"
        );
    }

    #[test]
    fn grouped_tampered_amount_breaks_commitment() {
        let data = bytes(GROUPED_HEX);
        let f = parse_proof_file(&data).unwrap();
        let p = parse_proof(f.proofs[1]).unwrap();
        let asset = p.asset_leaf().unwrap();
        let leaf_bytes = p.get(tlv::ASSET_LEAF).unwrap();
        let tp = parse_taproot_proof(p.get(tlv::INCLUSION_PROOF).unwrap()).unwrap();
        let cp = tp.parse_commitment().unwrap().unwrap();
        let anchor = verify_anchor(&p).unwrap();
        let mut bad = asset;
        bad.amount ^= 1;
        let key = reconstruct_output_key(&bad, leaf_bytes, &cp, &tp).unwrap();
        assert_ne!(key, anchor.taproot_output_key);
    }

    #[test]
    fn lineage_broken_when_reordered() {
        // Swapping the order breaks the prev_out → prior-anchor chain.
        let data = bytes(TRANSFER_HEX);
        let f = parse_proof_file(&data).unwrap();
        let mut proofs = f.parsed().unwrap();
        proofs.reverse();
        assert_eq!(verify_lineage(&proofs), Err(ProofError::LineageBroken));
    }

    #[test]
    fn transfer_split_output_committed_in_anchor() {
        // The split transfer output (proof[1]) commits its asset in its anchor:
        // tapd strips the split-commitment witness before hashing the leaf, and
        // the reconstructed taproot output key equals the on-chain key.
        let data = bytes(TRANSFER_HEX);
        let f = parse_proof_file(&data).unwrap();
        let proofs = f.parsed().unwrap();
        let anchor = verify_asset_inclusion(&proofs[1]).expect("split output committed on-chain");
        // The 250000 split output's on-chain x-only taproot key (from bitcoind).
        assert_eq!(
            hex::encode(anchor.taproot_output_key),
            "c74aed02a691d58a20c6e5e0401bd8bca5ce1cf9cf3053421162fc512648a537"
        );
    }

    #[test]
    fn split_output_uses_stripped_leaf() {
        // The committed leaf for a split output is NOT the raw asset-leaf bytes:
        // the split-commitment witness sub-record is removed first. Using the raw
        // leaf (no strip) must fail to reconstruct the on-chain key.
        let data = bytes(TRANSFER_HEX);
        let f = parse_proof_file(&data).unwrap();
        let p = parse_proof(f.proofs[1]).unwrap();
        let leaf_bytes = p.get(tlv::ASSET_LEAF).unwrap();
        assert!(
            split_committed_leaf_bytes(leaf_bytes).is_some(),
            "split output asset is a split-commitment witness"
        );
        // The genesis (mint) asset is not a split-commitment witness ⇒ raw leaf.
        let gdata = bytes(GENESIS_HEX);
        let gf = parse_proof_file(&gdata).unwrap();
        let g = parse_proof(gf.proofs[0]).unwrap();
        assert!(split_committed_leaf_bytes(g.get(tlv::ASSET_LEAF).unwrap()).is_none());
    }

    #[test]
    fn tampered_split_amount_breaks_commitment() {
        // Flip the split output's committed amount ⇒ reconstructed key ≠ on-chain.
        let data = bytes(TRANSFER_HEX);
        let f = parse_proof_file(&data).unwrap();
        let p = parse_proof(f.proofs[1]).unwrap();
        let asset = p.asset_leaf().unwrap();
        let leaf_bytes = p.get(tlv::ASSET_LEAF).unwrap();
        let tp = parse_taproot_proof(p.get(tlv::INCLUSION_PROOF).unwrap()).unwrap();
        let cp = tp.parse_commitment().unwrap().unwrap();
        let anchor = verify_anchor(&p).unwrap();
        let mut bad = asset;
        bad.amount ^= 1;
        let key = reconstruct_output_key(&bad, leaf_bytes, &cp, &tp).unwrap();
        assert_ne!(key, anchor.taproot_output_key);
    }

    #[test]
    fn tampered_amount_breaks_commitment() {
        // Flip the asset's committed amount ⇒ reconstructed key ≠ on-chain key.
        let data = bytes(GENESIS_HEX);
        let f = parse_proof_file(&data).unwrap();
        let p = parse_proof(f.proofs[0]).unwrap();
        let asset = p.asset_leaf().unwrap();
        let leaf_bytes = p.get(tlv::ASSET_LEAF).unwrap();
        let tp = parse_taproot_proof(p.get(tlv::INCLUSION_PROOF).unwrap()).unwrap();
        let cp = tp.parse_commitment().unwrap().unwrap();
        let mut bad = asset;
        bad.amount ^= 1;
        let anchor = verify_anchor(&p).unwrap();
        let key = reconstruct_output_key(&bad, leaf_bytes, &cp, &tp).unwrap();
        assert_ne!(key, anchor.taproot_output_key);
    }

    #[test]
    fn genesis_commitment_proof_decodes() {
        let data = bytes(GENESIS_HEX);
        let f = parse_proof_file(&data).unwrap();
        let p = parse_proof(f.proofs[0]).unwrap();
        let tp = parse_taproot_proof(p.get(tlv::INCLUSION_PROOF).unwrap()).unwrap();
        let cp = tp
            .parse_commitment()
            .expect("has commitment proof")
            .unwrap();

        // Inner proof: asset is in the AssetCommitment keyed by its asset_id.
        assert_eq!(cp.asset_proof.version, 0);
        assert_eq!(
            hex::encode(cp.asset_proof.asset_id),
            "ece4593a38bb4f57adc60e1b82395e81026a9eb5be4653d8a783feece52f6e6a"
        );
        // Outer proof: AssetCommitment is in a v2 TapCommitment.
        assert_eq!(cp.taproot_asset_proof.version, 2);
        // Single-leaf trees ⇒ 34-byte compressed MS-SMT proofs (decoded next).
        assert_eq!(cp.asset_proof.mssmt_proof.len(), 34);
        assert_eq!(cp.taproot_asset_proof.mssmt_proof.len(), 34);
    }

    #[test]
    fn transfer_split_amount_decodes() {
        // The 250000 split output's proof leaf carries that amount.
        let data = bytes(TRANSFER_HEX);
        let f = parse_proof_file(&data).unwrap();
        let proofs = f.parsed().unwrap();
        let last = proofs.last().unwrap().asset_leaf().expect("leaf");
        assert_eq!(last.amount, 250_000);
        // Same genesis ⇒ same asset_id across the lineage.
        assert_eq!(
            hex::encode(last.genesis.asset_id()),
            "ece4593a38bb4f57adc60e1b82395e81026a9eb5be4653d8a783feece52f6e6a"
        );
    }
}
