//! Assemble a tapd `proof.File` for an output sent to an **external** key.
//!
//! tapd only archives full proofs for keys it owns, so a send to a foreign
//! script key (a SatUSD lock anchor or burn sink) can't be exported directly.
//! But the local change output of the *same* anchor tx shares the identical input
//! lineage. We export the change output's proof file, drop its final transition,
//! and append the external output's transition (`new_proof_blob`) — completed with
//! the tx-wide chain-anchor records from the change transition, since the
//! response's blob carries only a placeholder tx-merkle proof before confirmation.

use bitcoin::hashes::{sha256, Hash, HashEngine};

/// Read an lnd BigSize varint at `*i` (big-endian, 1/3/5/9 bytes).
fn read_bigsize(b: &[u8], i: &mut usize) -> u64 {
    let first = b[*i];
    *i += 1;
    match first {
        0xff => {
            let v = u64::from_be_bytes(b[*i..*i + 8].try_into().unwrap());
            *i += 8;
            v
        }
        0xfe => {
            let v = u32::from_be_bytes(b[*i..*i + 4].try_into().unwrap()) as u64;
            *i += 4;
            v
        }
        0xfd => {
            let v = u16::from_be_bytes(b[*i..*i + 2].try_into().unwrap()) as u64;
            *i += 2;
            v
        }
        n => n as u64,
    }
}

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

/// Split a `proof.File` ("TAPF" + version + count + records) into single proofs.
pub fn split_proof_file(data: &[u8]) -> Vec<Vec<u8>> {
    let mut i = 8; // "TAPF" + u32 version
    let count = read_bigsize(data, &mut i);
    let mut proofs = Vec::with_capacity(count as usize);
    for _ in 0..count {
        let len = read_bigsize(data, &mut i) as usize;
        proofs.push(data[i..i + len].to_vec());
        i += len + 32; // skip proof + its sha256-chain checksum
    }
    proofs
}

/// Parse a single `proof.Proof` ("TAPP" + TLV stream) into (type, value) records.
fn parse_proof_records(blob: &[u8]) -> Vec<(u64, Vec<u8>)> {
    let mut i = 4; // skip "TAPP"
    let mut recs = Vec::new();
    while i < blob.len() {
        let ty = read_bigsize(blob, &mut i);
        let len = read_bigsize(blob, &mut i) as usize;
        recs.push((ty, blob[i..i + len].to_vec()));
        i += len;
    }
    recs
}

/// Re-encode (type, value) records into a "TAPP" proof, types strictly ascending.
fn encode_proof_records(mut recs: Vec<(u64, Vec<u8>)>) -> Vec<u8> {
    recs.sort_by_key(|(t, _)| *t);
    let mut out = b"TAPP".to_vec();
    for (ty, v) in recs {
        write_bigsize(ty, &mut out);
        write_bigsize(v.len() as u64, &mut out);
        out.extend_from_slice(&v);
    }
    out
}

/// Complete a pre-confirmation transition proof (`partial`, whose tx-merkle proof
/// is only a placeholder before the anchor confirms) by **overwriting** the
/// tx-wide chain-anchor records (block header 4, anchor tx 6, tx merkle proof 8,
/// block height 22) with a sibling output's confirmed proof from the same anchor
/// tx — those records are identical for every output of that tx.
pub fn complete_transition(partial: &[u8], confirmed_sibling: &[u8]) -> Vec<u8> {
    const ANCHOR_TYPES: [u64; 4] = [4, 6, 8, 22];
    let sib = parse_proof_records(confirmed_sibling);
    let mut recs: Vec<(u64, Vec<u8>)> = parse_proof_records(partial)
        .into_iter()
        .filter(|(t, _)| !ANCHOR_TYPES.contains(t))
        .collect();
    for t in ANCHOR_TYPES {
        if let Some((_, v)) = sib.iter().find(|(ty, _)| *ty == t) {
            recs.push((t, v.clone()));
        }
    }
    encode_proof_records(recs)
}

/// Re-encode a `proof.File` (version 0) from ordered single-proof blobs,
/// recomputing the sha256 hash chain `h_i = sha256(h_{i-1} || proof_i)`.
pub fn build_proof_file(proofs: &[Vec<u8>]) -> Vec<u8> {
    let mut out = b"TAPF".to_vec();
    out.extend_from_slice(&0u32.to_be_bytes());
    write_bigsize(proofs.len() as u64, &mut out);
    let mut prev = [0u8; 32];
    for p in proofs {
        write_bigsize(p.len() as u64, &mut out);
        out.extend_from_slice(p);
        let mut e = sha256::Hash::engine();
        e.input(&prev);
        e.input(p);
        let h = sha256::Hash::from_engine(e).to_byte_array();
        out.extend_from_slice(&h);
        prev = h;
    }
    out
}

/// Assemble the external output's full `proof.File`: take the local change
/// output's confirmed proof file (same anchor tx), drop its final transition, and
/// append the external output's (completed) transition.
pub fn assemble_external_proof_file(change_file: &[u8], external_transition: &[u8]) -> Vec<u8> {
    let mut proofs = split_proof_file(change_file);
    let change_transition = proofs.pop().expect("empty change proof file");
    let full = complete_transition(external_transition, &change_transition);
    proofs.push(full);
    build_proof_file(&proofs)
}
