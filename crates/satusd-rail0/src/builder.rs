//! The collapsed LOCK+SETTLE transaction builder (spec 02 §3.5).
//!
//! Composes one Bitcoin transaction carrying both legs of the swap:
//! the user's SatUSD moving at the TA layer, and the LP's BTC paying
//! the user at the BTC layer. tapd's external-anchor flow does the
//! TA-side heavy lifting:
//!
//! ```text
//! FundVirtualPsbt → SignVirtualPsbt
//!     → CommitVirtualPsbts(anchor template + LP input + payout)
//!     → [sign BTC inputs: LP wallet + tapd's lnd]
//!     → PublishAndLogTransfer
//! ```
//!
//! The anchor template reserves dust P2TR slots for every asset
//! anchor output (tapd rewrites their scripts at commit time) and
//! carries our real outputs untouched.

use bitcoin::absolute::LockTime;
use bitcoin::psbt::Psbt;
use bitcoin::transaction::Version;
use bitcoin::{Amount, OutPoint, ScriptBuf, Sequence, Transaction, TxIn, TxOut, Witness};
use satusd_tapd_client::assetwalletrpc as awrpc;
use satusd_tapd_client::{AssetWalletClient, TapChannel};

/// tapd's standard anchor output value.
pub const ANCHOR_DUST_SATS: u64 = 1_000;

/// Placeholder P2TR script for asset anchor slots; tapd replaces the
/// key with the real commitment key at commit time. Uses the tapd
/// NUMS x-coordinate, which is a valid x-only key.
fn anchor_placeholder_script() -> ScriptBuf {
    use bitcoin::key::TweakedPublicKey;
    let key = bitcoin::XOnlyPublicKey::from_slice(&crate::burn_key::TAPD_NUMS_X)
        .expect("NUMS x is a valid x-only key");
    ScriptBuf::new_p2tr_tweaked(TweakedPublicKey::dangerous_assume_tweaked(key))
}

/// The swap's non-TA legs. Everything about the TA anchor input and
/// the asset anchor outputs is parsed from the funded vPSBT itself.
pub struct AnchorTemplate {
    /// The LP's BTC input funding the payout.
    pub lp_outpoint: OutPoint,
    /// The LP input's previous output (needed by the LP's signer).
    pub lp_prev_txout: TxOut,
    /// The user's BTC payout (SwapPlan::user_sats).
    pub user_payout: TxOut,
}

/// The TA anchor input's on-chain facts, parsed from the funded
/// vPSBT's tap-proprietary input fields:
///
/// | type | content |
/// |------|---------|
/// | 112  | PrevID: outpoint(36, wire) ‖ asset_id(32) ‖ script_key |
/// | 113  | anchor output value (u64 BE) |
/// | 114  | anchor pkScript |
/// | 116  | anchor internal key (33-byte compressed) |
/// | 117  | anchor BIP-341 merkle root (32) |
#[derive(Clone, Debug)]
pub struct AnchorInInfo {
    pub outpoint: OutPoint,
    pub value_sats: u64,
    pub script: ScriptBuf,
    pub internal_key: bitcoin::XOnlyPublicKey,
    pub merkle_root: Option<[u8; 32]>,
}

/// Parse the anchor-input facts from a funded vPSBT.
pub fn vpsbt_anchor_input(funded_vpsbt: &[u8]) -> Result<AnchorInInfo, Box<dyn std::error::Error>> {
    const T_PREV_ID: u8 = 112;
    const T_ANCHOR_VALUE: u8 = 113;
    const T_ANCHOR_PK_SCRIPT: u8 = 114;
    const T_ANCHOR_INTERNAL_KEY: u8 = 116;
    const T_ANCHOR_MERKLE_ROOT: u8 = 117;

    let psbt = Psbt::deserialize(funded_vpsbt)?;
    let input = psbt.inputs.first().ok_or("funded vPSBT has no inputs")?;
    let get = |t: u8| {
        input
            .unknown
            .iter()
            .find(|(k, _)| k.type_value == t)
            .map(|(_, v)| v.clone())
    };

    let prev_id = get(T_PREV_ID).ok_or("vIn missing PrevID (type 112)")?;
    if prev_id.len() < 36 {
        return Err("PrevID too short".into());
    }
    // Txid stores wire byte order internally — no reversal needed.
    use bitcoin::hashes::Hash;
    let outpoint = OutPoint::new(
        bitcoin::Txid::from_byte_array(prev_id[..32].try_into().unwrap()),
        u32::from_le_bytes(prev_id[32..36].try_into().unwrap()),
    );

    let value_sats = get(T_ANCHOR_VALUE)
        .map(|v| u64::from_be_bytes(v.as_slice().try_into().unwrap()))
        .ok_or("vIn missing anchor value (type 113)")?;
    let script = ScriptBuf::from_bytes(
        get(T_ANCHOR_PK_SCRIPT).ok_or("vIn missing anchor pkScript (type 114)")?,
    );
    let ik = get(T_ANCHOR_INTERNAL_KEY).ok_or("vIn missing anchor internal key (type 116)")?;
    let internal_key = bitcoin::XOnlyPublicKey::from_slice(match ik.len() {
        33 => &ik[1..],
        32 => &ik[..],
        _ => return Err("bad anchor internal key length".into()),
    })?;
    let merkle_root = get(T_ANCHOR_MERKLE_ROOT).and_then(|v| <[u8; 32]>::try_from(v).ok());

    Ok(AnchorInInfo {
        outpoint,
        value_sats,
        script,
        internal_key,
        merkle_root,
    })
}

/// Per-anchor-output info extracted from a funded vPSBT. tapd stores
/// the BTC-anchor data in tap-proprietary unknown output fields (the
/// standard BIP-371 `tap_internal_key` on a *virtual* output is the
/// asset-level key — the wrong key for the anchor template):
///
/// | type | content |
/// |------|---------|
/// | 114  | anchor output index (u64 BE) |
/// | 115  | anchor internal key (33-byte compressed) |
/// | 116  | anchor BIP32 derivation (key = pubkey, val = fp ‖ path) |
/// | 117  | anchor taproot BIP32 derivation (key = x-only, val = BIP-371 layout) |
#[derive(Clone, Debug)]
pub struct AnchorOutInfo {
    pub internal_key: bitcoin::XOnlyPublicKey,
    pub bip32: Option<(bitcoin::secp256k1::PublicKey, Vec<u8>)>,
    pub tap_origin: Option<(bitcoin::XOnlyPublicKey, Vec<u8>)>,
}

fn parse_origin(
    val: &[u8],
) -> Option<(bitcoin::bip32::Fingerprint, bitcoin::bip32::DerivationPath)> {
    use bitcoin::bip32::{ChildNumber, DerivationPath, Fingerprint};
    if val.len() < 4 || !(val.len() - 4).is_multiple_of(4) {
        return None;
    }
    let fp = Fingerprint::from(<[u8; 4]>::try_from(&val[..4]).ok()?);
    let children: Vec<ChildNumber> = val[4..]
        .chunks(4)
        .map(|c| ChildNumber::from(u32::from_le_bytes(c.try_into().unwrap())))
        .collect();
    Some((fp, DerivationPath::from(children)))
}

/// Extract anchor info ordered by anchor output index 0..n.
pub fn vpsbt_anchor_info(
    funded_vpsbt: &[u8],
) -> Result<Vec<AnchorOutInfo>, Box<dyn std::error::Error>> {
    const T_INDEX: u8 = 114;
    const T_INTERNAL_KEY: u8 = 115;
    const T_BIP32: u8 = 116;
    const T_TAP_BIP32: u8 = 117;

    let psbt = Psbt::deserialize(funded_vpsbt)?;
    let mut keyed: Vec<(u64, AnchorOutInfo)> = Vec::new();
    for out in &psbt.outputs {
        let get = |t: u8| {
            out.unknown
                .iter()
                .find(|(k, _)| k.type_value == t)
                .map(|(k, v)| (k.key.clone(), v.clone()))
        };
        let idx = get(T_INDEX)
            .map(|(_, v)| u64::from_be_bytes(v.as_slice().try_into().unwrap()))
            .ok_or("vOut missing anchor output index (type 114)")?;
        let (_, ik) = get(T_INTERNAL_KEY).ok_or("vOut missing anchor internal key (type 115)")?;
        let internal_key = bitcoin::XOnlyPublicKey::from_slice(match ik.len() {
            33 => &ik[1..],
            32 => &ik[..],
            _ => return Err("bad anchor internal key length".into()),
        })?;
        keyed.push((
            idx,
            AnchorOutInfo {
                internal_key,
                bip32: get(T_BIP32).and_then(|(k, v)| {
                    Some((bitcoin::secp256k1::PublicKey::from_slice(&k).ok()?, v))
                }),
                tap_origin: get(T_TAP_BIP32)
                    .and_then(|(k, v)| Some((bitcoin::XOnlyPublicKey::from_slice(&k).ok()?, v))),
            },
        ));
    }
    keyed.sort_by_key(|(i, _)| *i);
    if keyed
        .iter()
        .enumerate()
        .any(|(want, (got, _))| *got != want as u64)
    {
        return Err("anchor output indexes are not contiguous from 0".into());
    }
    Ok(keyed.into_iter().map(|(_, info)| info).collect())
}

/// Build the BTC-level anchor template for `CommitVirtualPsbts`:
/// inputs = [TA anchor, LP], outputs = [anchor slots × n, user
/// payout]. Each anchor slot carries its internal key (BIP-371
/// output field) so tapd can compute the final commitment scripts.
/// LP change is requested from tapd via `anchor_change_output: add`.
pub fn build_anchor_template(
    t: &AnchorTemplate,
    anchor_in: &AnchorInInfo,
    anchor_keys: &[AnchorOutInfo],
) -> Psbt {
    let tx = Transaction {
        version: Version::TWO,
        lock_time: LockTime::ZERO,
        input: vec![
            TxIn {
                previous_output: anchor_in.outpoint,
                script_sig: ScriptBuf::new(),
                sequence: Sequence::ENABLE_RBF_NO_LOCKTIME,
                witness: Witness::new(),
            },
            TxIn {
                previous_output: t.lp_outpoint,
                script_sig: ScriptBuf::new(),
                sequence: Sequence::ENABLE_RBF_NO_LOCKTIME,
                witness: Witness::new(),
            },
        ],
        output: anchor_keys
            .iter()
            .map(|_| TxOut {
                value: Amount::from_sat(ANCHOR_DUST_SATS),
                script_pubkey: anchor_placeholder_script(),
            })
            .chain(std::iter::once(t.user_payout.clone()))
            .collect(),
    };
    let mut psbt = Psbt::from_unsigned_tx(tx).expect("template tx is unsigned");
    // tapd's input validation needs the TA anchor's taproot info up
    // front ("invalid anchor input info" otherwise).
    psbt.inputs[0].witness_utxo = Some(TxOut {
        value: Amount::from_sat(anchor_in.value_sats),
        script_pubkey: anchor_in.script.clone(),
    });
    psbt.inputs[0].tap_internal_key = Some(anchor_in.internal_key);
    if let Some(root) = anchor_in.merkle_root {
        use bitcoin::hashes::Hash;
        psbt.inputs[0].tap_merkle_root = Some(bitcoin::TapNodeHash::from_byte_array(root));
    }
    // The LP signer needs its witness UTXO.
    psbt.inputs[1].witness_utxo = Some(t.lp_prev_txout.clone());
    // Anchor slots carry the anchor internal key + both derivation
    // forms, mirroring the vPSBT's anchor data — tapd's publish-time
    // ValidateAnchorOutputs checks all three for equality.
    for (i, info) in anchor_keys.iter().enumerate() {
        psbt.outputs[i].tap_internal_key = Some(info.internal_key);
        if let Some((pk, val)) = &info.bip32 {
            if let Some(origin) = parse_origin(val) {
                psbt.outputs[i].bip32_derivation.insert(*pk, origin);
            }
        }
        if let Some((xonly, val)) = &info.tap_origin {
            // BIP-371 value layout: <compact hashes len><leaf hashes>
            // <fingerprint><path>; tapd anchors carry zero leaf hashes.
            if val.first() == Some(&0) {
                if let Some(origin) = parse_origin(&val[1..]) {
                    psbt.outputs[i]
                        .tap_key_origins
                        .insert(*xonly, (vec![], origin));
                }
            }
        }
    }
    psbt
}

/// Outcome of the commit step: the funded anchor PSBT awaiting BTC
/// signatures, plus the proofs-bearing virtual transactions to hand
/// to `PublishAndLogTransfer` afterwards.
pub struct CommittedSwap {
    pub anchor_psbt: Vec<u8>,
    pub virtual_psbts: Vec<Vec<u8>>,
    pub passive_asset_psbts: Vec<Vec<u8>>,
    pub change_output_index: i32,
    pub lnd_locked_utxos: Vec<satusd_tapd_client::taprpc::OutPoint>,
}

/// A funded (not yet signed) virtual transfer.
pub struct FundedTransfer {
    pub funded_psbt: Vec<u8>,
    pub passive_asset_psbts: Vec<Vec<u8>>,
}

impl FundedTransfer {
    /// The anchor-input facts tapd selected at funding time. The
    /// template MUST spend exactly this outpoint — guessing from
    /// ListUtxos goes stale the moment a previous swap moves the
    /// asset.
    pub fn anchor_input(&self) -> Result<AnchorInInfo, Box<dyn std::error::Error>> {
        vpsbt_anchor_input(&self.funded_psbt)
    }

    pub fn anchor_outputs(&self) -> Result<Vec<AnchorOutInfo>, Box<dyn std::error::Error>> {
        vpsbt_anchor_info(&self.funded_psbt)
    }
}

/// Fund the user's virtual transfer to the LP's TA address.
pub async fn fund(
    wallet: &mut AssetWalletClient<TapChannel>,
    lp_ta_addr: &str,
) -> Result<FundedTransfer, Box<dyn std::error::Error>> {
    let funded = wallet
        .fund_virtual_psbt(awrpc::FundVirtualPsbtRequest {
            template: Some(awrpc::fund_virtual_psbt_request::Template::Raw(
                awrpc::TxTemplate {
                    recipients: [(lp_ta_addr.to_string(), 0u64)].into_iter().collect(),
                    ..Default::default()
                },
            )),
            ..Default::default()
        })
        .await?
        .into_inner();
    Ok(FundedTransfer {
        funded_psbt: funded.funded_psbt,
        passive_asset_psbts: funded.passive_asset_psbts,
    })
}

/// Sign the funded transfer and commit it into an anchor template
/// built from `t` and the anchor info the funded vPSBT declares.
pub async fn sign_commit(
    wallet: &mut AssetWalletClient<TapChannel>,
    funded: FundedTransfer,
    t: &AnchorTemplate,
    sat_per_vbyte: u64,
) -> Result<CommittedSwap, Box<dyn std::error::Error>> {
    let anchor_in = funded.anchor_input()?;
    let anchor_keys = funded.anchor_outputs()?;
    let template = build_anchor_template(t, &anchor_in, &anchor_keys);

    let signed = wallet
        .sign_virtual_psbt(awrpc::SignVirtualPsbtRequest {
            funded_psbt: funded.funded_psbt.clone(),
        })
        .await?
        .into_inner();

    let committed = wallet
        .commit_virtual_psbts(awrpc::CommitVirtualPsbtsRequest {
            virtual_psbts: vec![signed.signed_psbt],
            passive_asset_psbts: funded.passive_asset_psbts,
            anchor_psbt: template.serialize(),
            anchor_change_output: Some(
                awrpc::commit_virtual_psbts_request::AnchorChangeOutput::Add(true),
            ),
            fees: Some(awrpc::commit_virtual_psbts_request::Fees::SatPerVbyte(
                sat_per_vbyte,
            )),
            ..Default::default()
        })
        .await?
        .into_inner();

    Ok(CommittedSwap {
        anchor_psbt: committed.anchor_psbt,
        virtual_psbts: committed.virtual_psbts,
        passive_asset_psbts: committed.passive_asset_psbts,
        change_output_index: committed.change_output_index,
        lnd_locked_utxos: committed.lnd_locked_utxos,
    })
}

/// Publish the fully signed anchor transaction and log the transfer
/// in tapd's database. `signed_anchor_psbt` must carry final
/// signatures for every BTC input.
pub async fn publish(
    wallet: &mut AssetWalletClient<TapChannel>,
    swap: CommittedSwap,
    signed_anchor_psbt: Vec<u8>,
) -> Result<satusd_tapd_client::taprpc::SendAssetResponse, Box<dyn std::error::Error>> {
    let resp = wallet
        .publish_and_log_transfer(awrpc::PublishAndLogRequest {
            anchor_psbt: signed_anchor_psbt,
            virtual_psbts: swap.virtual_psbts,
            passive_asset_psbts: swap.passive_asset_psbts,
            change_output_index: swap.change_output_index,
            lnd_locked_utxos: swap.lnd_locked_utxos,
            ..Default::default()
        })
        .await?
        .into_inner();
    Ok(resp)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bitcoin::hashes::Hash;

    #[test]
    fn template_shape() {
        let payout = TxOut {
            value: Amount::from_sat(999_300),
            script_pubkey: anchor_placeholder_script(),
        };
        let nums = bitcoin::XOnlyPublicKey::from_slice(&crate::burn_key::TAPD_NUMS_X).unwrap();
        let t = AnchorTemplate {
            lp_outpoint: OutPoint::new(bitcoin::Txid::all_zeros(), 1),
            lp_prev_txout: TxOut {
                value: Amount::from_sat(2_000_000),
                script_pubkey: anchor_placeholder_script(),
            },
            user_payout: payout.clone(),
        };
        let anchor_in = AnchorInInfo {
            outpoint: OutPoint::new(bitcoin::Txid::all_zeros(), 0),
            value_sats: 1_000,
            script: anchor_placeholder_script(),
            internal_key: nums,
            merkle_root: Some([0x11; 32]),
        };
        let info = AnchorOutInfo {
            internal_key: nums,
            bip32: None,
            tap_origin: None,
        };
        let psbt = build_anchor_template(&t, &anchor_in, &[info.clone(), info]);
        let tx = &psbt.unsigned_tx;
        assert_eq!(tx.input.len(), 2);
        assert_eq!(tx.output.len(), 3, "2 anchor slots + payout");
        assert_eq!(tx.output[0].value.to_sat(), ANCHOR_DUST_SATS);
        assert_eq!(tx.output[2], payout);
        assert!(psbt.inputs[0].tap_internal_key.is_some());
        assert!(psbt.inputs[0].tap_merkle_root.is_some());
        assert!(psbt.inputs[1].witness_utxo.is_some(), "LP signer needs it");
        assert_eq!(psbt.outputs[0].tap_internal_key, Some(nums));
        assert_eq!(psbt.outputs[1].tap_internal_key, Some(nums));
        assert_eq!(psbt.outputs[2].tap_internal_key, None, "payout untouched");
        // Round-trips through serialization for the gRPC boundary.
        let bytes = psbt.serialize();
        assert_eq!(Psbt::deserialize(&bytes).unwrap(), psbt);
    }
}
