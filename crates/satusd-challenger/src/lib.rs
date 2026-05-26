//! SatUSD challenger — detection mode (PRD §14.12 M5, §5.D18).
//!
//! In the MVP (Mock / Multisig stage) the challenger does **detection + alert**,
//! not on-chain dispute. Given a submitted `ReserveClaim`, it independently:
//!
//! - confirms the operator uploaded the Live DA bundle (§10.1) — DA-01;
//! - recomputes `live_da_root` from the bundle and compares it to the claim's
//!   committed root (ADR-008) — DA-02;
//! - checks the bundle carries the sections needed to recompute the claim — DA-03
//!   (oracle messages, 0x08) / DA-06 (lineage, see [`LINEAGE_SECTION`]);
//! - re-runs the TA lineage verifier (DL-23) on a proof from the DA — a fake
//!   lineage is recomputed and flagged (§14.12 acceptance).
//!
//! All checks are independent of the state node: the challenger never trusts the
//! operator's `asset_proof_hash` or the node's acceptance. Multi-mirror fetch
//! (DA-04/05), Prometheus, and the dashboard are operational layers deferred past
//! the verification core.

use satusd_da::{section, LiveDABundle};
use satusd_ta_proof::{parse_proof_file, verify_lineage};

/// The DA section the challenger re-runs lineage against. PRD §10.3 reserves 0x03
/// for lock-time TA lineage proofs; in the claim bundle the genesis→burn lineage
/// the challenger re-verifies travels in the universe-burn-proofs section (0x07),
/// so DA-06 ("missing lineage") maps here (see ADR-008).
pub const LINEAGE_SECTION: u8 = section::UNIVERSE_BURN_PROOFS;

/// A detection finding. Each maps to a §13.4 DA adversarial case or the §14.12
/// lineage acceptance criterion.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Alert {
    /// DA-01: the operator submitted a claim without uploading the Live DA bundle.
    DaBundleMissing,
    /// DA-02: the bundle does not hash to the claim's committed `live_da_root`.
    LiveDaRootMismatch {
        committed: [u8; 32],
        recomputed: [u8; 32],
    },
    /// DA-03 (0x08) / DA-06 (lineage): a section needed to recompute is absent.
    MissingSection(u8),
    /// The re-run TA lineage verifier rejected a proof from the DA (fake lineage).
    LineageInvalid,
}

/// The outcome of inspecting one claim. Empty = nothing to flag.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Verdict {
    pub alerts: Vec<Alert>,
}

impl Verdict {
    pub fn clean(&self) -> bool {
        self.alerts.is_empty()
    }
}

/// Sections a challenger needs to recompute a claim (subset of §10.3 present in
/// the claim bundle): redeem intents, payout confirmations, the lineage/burn
/// proofs, and the oracle messages.
const REQUIRED_SECTIONS: [u8; 4] = [
    section::REDEEM_INTENTS,
    section::BTC_PAYOUT_CONFIRMATIONS,
    LINEAGE_SECTION,
    section::ORACLE_MESSAGES,
];

/// Inspect a submitted claim against the DA the operator published.
///
/// `committed_live_da_root` is the claim's on-chain `live_da_root`; `bundle` is the
/// Live DA bundle fetched from the mirrors (`None` = not uploaded). DA-layer
/// detection only — see [`recompute_lineage`] for the deep lineage re-run.
pub fn inspect(committed_live_da_root: [u8; 32], bundle: Option<&LiveDABundle>) -> Verdict {
    let mut alerts = Vec::new();

    let Some(bundle) = bundle else {
        alerts.push(Alert::DaBundleMissing); // DA-01
        return Verdict { alerts };
    };

    let recomputed = bundle.live_da_root();
    if recomputed != committed_live_da_root {
        alerts.push(Alert::LiveDaRootMismatch {
            committed: committed_live_da_root,
            recomputed,
        }); // DA-02
    }

    for id in REQUIRED_SECTIONS {
        if !bundle.has_section(id) {
            alerts.push(Alert::MissingSection(id)); // DA-03 (0x08) / DA-06 (lineage)
        }
    }

    Verdict { alerts }
}

/// Independently re-run the TA lineage verifier (DL-23) on a tapd `proof.File`
/// from the DA (the lineage section). Returns `LineageInvalid` if it does not
/// verify — the challenger recomputes a fake lineage and flags it (§14.12).
pub fn recompute_lineage(proof_file_bytes: &[u8]) -> Result<(), Alert> {
    let file = parse_proof_file(proof_file_bytes).map_err(|_| Alert::LineageInvalid)?;
    let proofs = file.parsed().map_err(|_| Alert::LineageInvalid)?;
    verify_lineage(&proofs).map_err(|_| Alert::LineageInvalid)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use satusd_da::Section;

    fn bytes(hex_str: &str) -> Vec<u8> {
        hex::decode(hex_str.trim()).unwrap()
    }

    /// A bundle carrying all sections the challenger requires.
    fn full_bundle() -> LiveDABundle {
        LiveDABundle::new(
            [0x11; 32],
            7,
            REQUIRED_SECTIONS
                .iter()
                .map(|&id| Section {
                    id,
                    content: vec![id; 8],
                })
                .collect(),
        )
    }

    #[test]
    fn clean_when_bundle_matches_committed_root() {
        let b = full_bundle();
        let v = inspect(b.live_da_root(), Some(&b));
        assert!(v.clean(), "{v:?}");
    }

    #[test]
    fn da01_missing_bundle() {
        let v = inspect([0x22; 32], None);
        assert_eq!(v.alerts, vec![Alert::DaBundleMissing]);
    }

    #[test]
    fn da02_root_mismatch() {
        let b = full_bundle();
        let committed = [0x00; 32]; // not the bundle's actual root
        let v = inspect(committed, Some(&b));
        assert_eq!(
            v.alerts,
            vec![Alert::LiveDaRootMismatch {
                committed,
                recomputed: b.live_da_root(),
            }]
        );
    }

    #[test]
    fn da03_missing_oracle_section() {
        let mut b = full_bundle();
        b.sections.retain(|s| s.id != section::ORACLE_MESSAGES);
        // Recompute the committed root to isolate the missing-section alert.
        let v = inspect(b.live_da_root(), Some(&b));
        assert_eq!(
            v.alerts,
            vec![Alert::MissingSection(section::ORACLE_MESSAGES)]
        );
    }

    #[test]
    fn da06_missing_lineage_section() {
        let mut b = full_bundle();
        b.sections.retain(|s| s.id != LINEAGE_SECTION);
        let v = inspect(b.live_da_root(), Some(&b));
        assert_eq!(v.alerts, vec![Alert::MissingSection(LINEAGE_SECTION)]);
    }

    #[test]
    fn lineage_recompute_accepts_real_burn_proof() {
        // The real grouped SatUSD burn-to-sink proof verifies independently.
        let data = bytes(include_str!(
            "../../../integration/lineage_vectors/burn_to_sink.hex"
        ));
        assert_eq!(recompute_lineage(&data), Ok(()));
    }

    #[test]
    fn lineage_recompute_flags_fake_lineage() {
        // Tamper a byte in the middle of a real proof → the lineage no longer
        // verifies → the challenger flags it (§14.12 fake-lineage acceptance).
        let mut data = bytes(include_str!(
            "../../../integration/lineage_vectors/burn_to_sink.hex"
        ));
        let mid = data.len() / 2;
        data[mid] ^= 0xff;
        assert_eq!(recompute_lineage(&data), Err(Alert::LineageInvalid));
    }
}
