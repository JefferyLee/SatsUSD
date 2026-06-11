//! dlcspecs TLV serialization for `oracle_announcement` and
//! `oracle_attestation` (Messaging.md / Oracle.md wire formats).
//!
//! TLV = bigsize(type) ‖ bigsize(len) ‖ value, LN conventions.
//! Cross-validation against an independent dlcspecs implementation
//! (rust-dlc) is the FR-5 acceptance's outstanding leg; until then
//! these encodings are pinned by vectors and internally consumed by
//! Rail-1.

use satusd_rail::encode::{DecodeError, Decoder, Encoder};

pub const TYPE_ORACLE_EVENT: u64 = 55330;
pub const TYPE_ORACLE_ANNOUNCEMENT: u64 = 55332;
pub const TYPE_ORACLE_ATTESTATION: u64 = 55400;
pub const TYPE_DIGIT_DECOMPOSITION_DESCRIPTOR: u64 = 55306;

fn tlv(type_id: u64, value: Vec<u8>) -> Vec<u8> {
    let mut out = Encoder::new();
    out.put_bigsize(type_id);
    out.put_bigsize(value.len() as u64);
    out.put_raw(&value);
    out.into_bytes()
}

/// `digit_decomposition_event_descriptor_v0`.
pub fn digit_descriptor(
    base: u16,
    is_signed: bool,
    unit: &str,
    precision: i32,
    nb_digits: u16,
) -> Vec<u8> {
    let mut v = Encoder::new();
    v.put_u16(base);
    v.put_bool(is_signed);
    v.put_string(unit);
    v.put_i32(precision);
    v.put_u16(nb_digits);
    tlv(TYPE_DIGIT_DECOMPOSITION_DESCRIPTOR, v.into_bytes())
}

/// `oracle_event_v0`: nonces, maturity, descriptor, event_id.
pub fn oracle_event(
    nonces: &[[u8; 32]],
    maturity_unix: u32,
    descriptor_tlv: &[u8],
    event_id: &str,
) -> Vec<u8> {
    let mut v = Encoder::new();
    v.put_u16(nonces.len() as u16);
    for n in nonces {
        v.put_bytes32(n);
    }
    v.put_u32(maturity_unix);
    v.put_raw(descriptor_tlv);
    v.put_string(event_id);
    tlv(TYPE_ORACLE_EVENT, v.into_bytes())
}

/// `oracle_announcement_v0`: signature ‖ pubkey ‖ oracle_event TLV.
pub fn oracle_announcement(
    announcement_sig: &[u8; 64],
    oracle_pubkey: &[u8; 32],
    oracle_event_tlv: &[u8],
) -> Vec<u8> {
    let mut v = Encoder::new();
    v.put_raw(announcement_sig);
    v.put_bytes32(oracle_pubkey);
    v.put_raw(oracle_event_tlv);
    tlv(TYPE_ORACLE_ANNOUNCEMENT, v.into_bytes())
}

/// A decoded `oracle_attestation_v0` — the challenger-side view
/// (spec 05: a conflicting pair of these IS the equivocation
/// evidence artifact).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParsedAttestation {
    pub event_id: String,
    pub oracle_pubkey: [u8; 32],
    /// 64-byte BIP-340 signatures, one per digit, MSB-first.
    pub signatures: Vec<[u8; 64]>,
    /// Outcome strings, parallel to `signatures`.
    pub outcomes: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParseError {
    Decode(DecodeError),
    WrongType(u64),
    /// Declared value length disagrees with the content read.
    LengthMismatch,
}

impl From<DecodeError> for ParseError {
    fn from(e: DecodeError) -> Self {
        ParseError::Decode(e)
    }
}

/// Strict parse of an `oracle_attestation` TLV (the exact mirror of
/// `oracle_attestation`); trailing bytes are rejected.
pub fn parse_attestation(bytes: &[u8]) -> Result<ParsedAttestation, ParseError> {
    let mut d = Decoder::new(bytes);
    let type_id = d.bigsize()?;
    if type_id != TYPE_ORACLE_ATTESTATION {
        return Err(ParseError::WrongType(type_id));
    }
    let value = d.varbytes()?;
    if !d.done() {
        return Err(ParseError::LengthMismatch);
    }

    let mut v = Decoder::new(value);
    let event_id = v.string()?.to_string();
    let oracle_pubkey = v.bytes32()?;
    let count = v.u16()? as usize;
    let mut signatures = Vec::with_capacity(count);
    for _ in 0..count {
        signatures.push(v.take(64)?.try_into().unwrap());
    }
    let mut outcomes = Vec::with_capacity(count);
    for _ in 0..count {
        outcomes.push(v.string()?.to_string());
    }
    if !v.done() {
        return Err(ParseError::LengthMismatch);
    }
    Ok(ParsedAttestation {
        event_id,
        oracle_pubkey,
        signatures,
        outcomes,
    })
}

/// `oracle_attestation_v0`: event_id ‖ pubkey ‖ sigs ‖ outcomes.
pub fn oracle_attestation(
    event_id: &str,
    oracle_pubkey: &[u8; 32],
    signatures: &[[u8; 64]],
    outcomes: &[&str],
) -> Vec<u8> {
    assert_eq!(signatures.len(), outcomes.len());
    let mut v = Encoder::new();
    v.put_string(event_id);
    v.put_bytes32(oracle_pubkey);
    v.put_u16(signatures.len() as u16);
    for s in signatures {
        v.put_raw(s);
    }
    for o in outcomes {
        v.put_string(o);
    }
    tlv(TYPE_ORACLE_ATTESTATION, v.into_bytes())
}
