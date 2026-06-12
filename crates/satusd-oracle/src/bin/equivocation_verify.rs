//! Equivocation verifier (spec 05 §2): given two `oracle_attestation`
//! TLVs in hex, run `verify_equivocation` and print the proof as JSON.
//! Exits non-zero on any verification failure — the artifact the slash
//! drill (FR-10) consumes.
//!
//! ```text
//! equivocation-verify <att_a.hex> <att_b.hex>
//! ```
//!
//! Each input is a path to a file containing the TLV as a single hex
//! string (trailing whitespace ignored). The proof JSON carries the
//! extracted oracle secret — anyone re-running the tool gets the same
//! bytes from the same inputs (self-certifying, spec 05 §2).

use std::process::ExitCode;

use satusd_oracle::equivocation::{verify_equivocation, EquivocationError};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let (Some(a_path), Some(b_path)) = (args.get(1), args.get(2)) else {
        eprintln!("usage: {} <att_a.hex> <att_b.hex>", args[0]);
        return ExitCode::from(2);
    };
    let a = match read_tlv(a_path) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("read {a_path}: {e}");
            return ExitCode::from(2);
        }
    };
    let b = match read_tlv(b_path) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("read {b_path}: {e}");
            return ExitCode::from(2);
        }
    };

    match verify_equivocation(&a, &b) {
        Ok(proof) => {
            println!(
                "{{\"event_id\":\"{}\",\"oracle_pubkey\":\"{}\",\"digit_index\":{},\"extracted_secret\":\"{}\"}}",
                proof.event_id,
                hex_encode(&proof.oracle_pubkey),
                proof.digit_index,
                hex_encode(&proof.extracted_secret),
            );
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("not a valid equivocation: {}", describe(e));
            ExitCode::from(1)
        }
    }
}

fn describe(e: EquivocationError) -> &'static str {
    match e {
        EquivocationError::Parse(_) => "parse error in one of the TLVs",
        EquivocationError::NotComparable => "different event/oracle/digit-count — not comparable",
        EquivocationError::NoConflict => "no digit signed two ways — not an equivocation",
        EquivocationError::BadOutcome => "outcome string is not a valid base-2 digit",
        EquivocationError::BadSignature => "signature fails BIP-340 or nonce points differ",
        EquivocationError::ExtractionFailed => "key extraction did not reproduce the oracle pubkey",
    }
}

fn read_tlv(path: &str) -> Result<Vec<u8>, String> {
    let raw = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    hex_decode(raw.trim()).map_err(|e| e.to_string())
}

fn hex_encode(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn hex_decode(s: &str) -> Result<Vec<u8>, String> {
    if !s.len().is_multiple_of(2) {
        return Err("odd hex length".into());
    }
    (0..s.len() / 2)
        .map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).map_err(|e| e.to_string()))
        .collect()
}
