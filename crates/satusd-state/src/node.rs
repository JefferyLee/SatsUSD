//! In-memory state node: holds the live `StateRoot` plus the six SMTs and the
//! issuer/claim records, and *executes* transitions end-to-end (generating its
//! own membership/exclusion proofs, applying the §8.1 executor, committing the
//! tree mutations, and self-checking that the committed roots match).
//!
//! This is the runnable accounting node the §7.3 HTTP API wraps. Callers pass
//! the business inputs (records + verified facts) in the transition witnesses;
//! the node fills in the SMT proof fields itself.
//!
//! Not yet wired: RECLAIM_STALE_CLAIM and multi-redemption batching depend on
//! claim submission (the ReserveClaim flow), which is a later chunk; the
//! `verify_reclaim_stale_claim` verifier already exists for that.

use std::collections::HashMap;

use satusd_crypto::smt::SparseMerkleTree;
use satusd_crypto::state::state_root_hash;
use satusd_types::derive::{issuer_position_hash, lock_record_hash, redemption_nullifier};
use satusd_types::types::{IssuerPosition, StateRoot};

use crate::redeem::SET_MEMBER;
use crate::{mint, redeem, registry};

#[derive(Debug, PartialEq, Eq)]
pub enum NodeError {
    Mint(mint::MintRejectReason),
    Redeem(redeem::RedeemRejectReason),
    Registry(registry::RegistryRejectReason),
    UnknownIssuer,
    /// The node's committed tree roots disagree with the executor's post-state —
    /// an internal invariant break (should never happen).
    InvariantViolation,
}

#[derive(Clone)]
pub struct StateNode {
    state: StateRoot,
    issuer_tree: SparseMerkleTree,
    issuers: HashMap<[u8; 32], IssuerPosition>,
    pending_claim_tree: SparseMerkleTree,
    lock_record_tree: SparseMerkleTree,
    lock_consumed_tree: SparseMerkleTree,
    lock_refund_tree: SparseMerkleTree,
    nullifier_tree: SparseMerkleTree,
}

impl StateNode {
    /// Fresh node: empty SMTs, epoch 0, the given asset family + oracle context.
    pub fn genesis(
        satusd_asset_family_id: [u8; 32],
        oracle_set_hash: [u8; 32],
        oracle_set_epoch: u64,
        latest_oracle_price_e8: u64,
    ) -> Self {
        let empty = SparseMerkleTree::new().root();
        let state = StateRoot {
            protocol_version: 1,
            state_epoch: 0,
            prev_state_root: [0; 32],
            transition_type: 0,
            satusd_asset_family_id,
            sat_usd_supply_atoms: 0,
            reserve_btc_sats: 0,
            reserved_pending_claim_sats: 0,
            collateral_ratio_ppm: 0,
            emergency_tier: 0,
            oracle_set_hash,
            oracle_set_epoch,
            latest_oracle_epoch_seen: oracle_set_epoch,
            latest_oracle_price_e8,
            issuer_positions_root: empty,
            operator_registry_root: empty,
            lock_record_root: empty,
            lock_consumed_root: empty,
            lock_refund_root: empty,
            redemption_nullifier_root: empty,
            pending_claim_root: empty,
            live_da_root: empty,
            archival_da_root: empty,
            l1_anchor_hash: [0; 32],
            l1_anchor_height: 0,
            l1_anchor_mtp: 0,
            l1_anchor_chain_time: 0,
        };
        StateNode {
            state,
            issuer_tree: SparseMerkleTree::new(),
            issuers: HashMap::new(),
            pending_claim_tree: SparseMerkleTree::new(),
            lock_record_tree: SparseMerkleTree::new(),
            lock_consumed_tree: SparseMerkleTree::new(),
            lock_refund_tree: SparseMerkleTree::new(),
            nullifier_tree: SparseMerkleTree::new(),
        }
    }

    pub fn state(&self) -> &StateRoot {
        &self.state
    }

    pub fn state_root_hash(&self) -> [u8; 32] {
        state_root_hash(&self.state)
    }

    pub fn issuer(&self, issuer_id: &[u8; 32]) -> Option<&IssuerPosition> {
        self.issuers.get(issuer_id)
    }

    /// Commit a candidate post-state, asserting the node's trees produce its roots.
    fn commit(&mut self, new_state: StateRoot) -> Result<[u8; 32], NodeError> {
        let ok = self.issuer_tree.root() == new_state.issuer_positions_root
            && self.pending_claim_tree.root() == new_state.pending_claim_root
            && self.lock_record_tree.root() == new_state.lock_record_root
            && self.lock_consumed_tree.root() == new_state.lock_consumed_root
            && self.lock_refund_tree.root() == new_state.lock_refund_root
            && self.nullifier_tree.root() == new_state.redemption_nullifier_root;
        if !ok {
            return Err(NodeError::InvariantViolation);
        }
        self.state = new_state;
        Ok(self.state_root_hash())
    }

    /// ISSUER_REGISTER.
    pub fn issuer_register(&mut self, new_issuer: IssuerPosition) -> Result<[u8; 32], NodeError> {
        let proof = self.issuer_tree.prove(&new_issuer.issuer_id);
        let new_state = registry::apply_issuer_register(&self.state, &new_issuer, &proof)
            .map_err(NodeError::Registry)?;
        self.issuer_tree
            .insert(new_issuer.issuer_id, &issuer_position_hash(&new_issuer));
        self.issuers.insert(new_issuer.issuer_id, new_issuer);
        self.commit(new_state)
    }

    /// MINT_COMMIT.
    pub fn mint_commit(
        &mut self,
        issuer_id: [u8; 32],
        w: &mint::MintCommitWitness,
    ) -> Result<[u8; 32], NodeError> {
        let prev_issuer = self
            .issuers
            .get(&issuer_id)
            .ok_or(NodeError::UnknownIssuer)?
            .clone();
        let proof = self.issuer_tree.prove(&issuer_id);
        let new_state = mint::apply_mint_commit(&self.state, &prev_issuer, &proof, w)
            .map_err(NodeError::Mint)?;
        let new_issuer = mint::issuer_after_commit(&prev_issuer, w).map_err(NodeError::Mint)?;
        self.issuer_tree
            .insert(issuer_id, &issuer_position_hash(&new_issuer));
        self.issuers.insert(issuer_id, new_issuer);
        self.commit(new_state)
    }

    /// MINT_FINALIZE.
    pub fn mint_finalize(
        &mut self,
        issuer_id: [u8; 32],
        w: &mint::MintFinalizeWitness,
    ) -> Result<[u8; 32], NodeError> {
        let prev_issuer = self
            .issuers
            .get(&issuer_id)
            .ok_or(NodeError::UnknownIssuer)?
            .clone();
        let proof = self.issuer_tree.prove(&issuer_id);
        let new_state = mint::apply_mint_finalize(&self.state, &prev_issuer, &proof, w)
            .map_err(NodeError::Mint)?;
        let new_issuer = mint::issuer_after_finalize(&prev_issuer, w.requested_mint_atoms)
            .map_err(NodeError::Mint)?;
        self.issuer_tree
            .insert(issuer_id, &issuer_position_hash(&new_issuer));
        self.issuers.insert(issuer_id, new_issuer);
        self.commit(new_state)
    }

    /// REDEEM_FAST_LOCK. `w`'s proof field is filled by the node.
    pub fn redeem_lock(&mut self, mut w: redeem::RedeemLockWitness) -> Result<[u8; 32], NodeError> {
        let lr_hash = lock_record_hash(&w.lock_record);
        w.lock_exclusion_proof = self.lock_record_tree.prove(&lr_hash);
        let new_state = redeem::apply_redeem_lock(&self.state, &w).map_err(NodeError::Redeem)?;
        self.lock_record_tree.insert(lr_hash, &SET_MEMBER);
        self.commit(new_state)
    }

    /// REDEEM_FAST_FINALIZE. `w`'s proof fields are filled by the node.
    pub fn redeem_finalize(
        &mut self,
        mut w: redeem::RedeemFinalizeWitness,
    ) -> Result<[u8; 32], NodeError> {
        let lr_hash = lock_record_hash(&w.lock_record);
        let nf = redemption_nullifier(
            &w.lock_record.lock_anchor_outpoint,
            &w.lock_record.lock_script_key,
            &w.lock_record.redeem_intent_hash,
        );
        w.lock_membership_proof = self.lock_record_tree.prove(&lr_hash);
        w.consumed_exclusion_proof = self.lock_consumed_tree.prove(&lr_hash);
        w.refund_exclusion_proof = self.lock_refund_tree.prove(&lr_hash);
        w.nullifier_exclusion_proof = self.nullifier_tree.prove(&nf);
        let new_state =
            redeem::apply_redeem_finalize(&self.state, &w).map_err(NodeError::Redeem)?;
        self.lock_consumed_tree.insert(lr_hash, &SET_MEMBER);
        self.nullifier_tree.insert(nf, &SET_MEMBER);
        self.commit(new_state)
    }

    /// LOCK_REFUND. `w`'s proof fields are filled by the node.
    pub fn lock_refund(&mut self, mut w: redeem::LockRefundWitness) -> Result<[u8; 32], NodeError> {
        let lr_hash = lock_record_hash(&w.lock_record);
        w.lock_membership_proof = self.lock_record_tree.prove(&lr_hash);
        w.consumed_exclusion_proof = self.lock_consumed_tree.prove(&lr_hash);
        w.refund_exclusion_proof = self.lock_refund_tree.prove(&lr_hash);
        let new_state = redeem::apply_lock_refund(&self.state, &w).map_err(NodeError::Redeem)?;
        self.lock_refund_tree.insert(lr_hash, &SET_MEMBER);
        self.commit(new_state)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use satusd_crypto::nums::{derive_nums_key, tap_tweak};
    use satusd_types::derive::{lock_tweak, redeem_intent_hash};
    use satusd_types::types::{
        BtcHtlcPayoutRecord, IssuerStatus, LockFinalizeRecord, LockRecord, OutPoint, RedeemIntent,
    };
    use secp256k1::{Message, Secp256k1, SecretKey};

    const PRICE_50K: u64 = 5_000_000_000_000;
    const FAMILY: [u8; 32] = [0x01; 32];
    const ISSUER_ID: [u8; 32] = [0xaa; 32];
    const OPERATOR: [u8; 32] = [0x20; 32];
    const PREIMAGE: [u8; 32] = [0x55; 32];
    const META: [u8; 32] = [0xef; 32];
    const DEPOSIT_TXID: [u8; 32] = [0xcd; 32];

    fn keys() -> [SecretKey; 3] {
        [
            SecretKey::from_byte_array([0x11; 32]).unwrap(),
            SecretKey::from_byte_array([0x22; 32]).unwrap(),
            SecretKey::from_byte_array([0x33; 32]).unwrap(),
        ]
    }

    fn pubkeys() -> Vec<[u8; 33]> {
        let secp = Secp256k1::new();
        keys()
            .iter()
            .map(|k| k.public_key(&secp).serialize())
            .collect()
    }

    fn issuer() -> IssuerPosition {
        IssuerPosition {
            issuer_id: ISSUER_ID,
            status: IssuerStatus::Active,
            multisig_pubkeys: pubkeys(),
            multisig_threshold: 2,
            reserve_deposits_sats: 0,
            minted_satusd_atoms: 0,
            pending_mint_atoms: 0,
            collateral_ratio_ppm: 0,
            last_deposit_txid: None,
            freeze_reason: None,
            registered_at_height: 100,
            pending_mint_commitment: None,
        }
    }

    fn commit_witness() -> mint::MintCommitWitness {
        let requested_mint_atoms = 100_000_000u64;
        let deposit_sats = 4_000_000_000u64;
        let sighash = satusd_types::derive::mint_request_sighash(
            &ISSUER_ID,
            requested_mint_atoms,
            deposit_sats,
            &DEPOSIT_TXID,
            &META,
        );
        let secp = Secp256k1::new();
        let signatures = keys()[..2]
            .iter()
            .zip(pubkeys())
            .map(|(sk, pk)| mint::MultisigSig {
                signer_pubkey: pk,
                signature: secp
                    .sign_ecdsa(Message::from_digest(sighash), sk)
                    .serialize_compact(),
            })
            .collect();
        mint::MintCommitWitness {
            issuer_id: ISSUER_ID,
            requested_mint_atoms,
            deposit_txid: DEPOSIT_TXID,
            deposit_sats,
            deposit_confirmations: 6,
            deposit_to_reserve: true,
            asset_metadata_commitment: META,
            signatures,
            oracle_price_e8: PRICE_50K,
        }
    }

    fn redeem_intent() -> RedeemIntent {
        RedeemIntent {
            version: 1,
            network: 0,
            redemption_id: [0x30; 32],
            satusd_asset_family_id: FAMILY,
            amount_satusd_atoms: 100_000_000,
            user_btc_refund_pubkey: [0x31; 32],
            user_btc_claim_pubkey: [0x32; 32],
            user_asset_refund_key: derive_nums_key("node-user-refund", &[]),
            operator_id: Some(OPERATOR),
            mode: redeem::MODE_FAST_OPERATOR,
            payment_hash: satusd_types::sha256(&[&PREIMAGE]),
            asset_lock_csv_delta: 288,
            btc_htlc_csv_delta: 144,
            max_operator_fee_bps: 50,
            l1_anchor_height: 840_000,
            l1_anchor_hash: [0x33; 32],
            expiry_height: 900_000,
            nonce: [0x34; 32],
        }
    }

    fn lock_record(it: &RedeemIntent) -> LockRecord {
        let rih = redeem_intent_hash(it);
        LockRecord {
            lock_record_version: 1,
            redeem_intent_hash: rih,
            lock_anchor_outpoint: OutPoint {
                txid: [0x40; 32],
                vout: 1,
            },
            lock_anchor_txid: [0x40; 32],
            lock_script_key: tap_tweak(
                &it.user_asset_refund_key,
                &lock_tweak(&rih, &it.payment_hash),
            ),
            lock_amount_atoms: it.amount_satusd_atoms,
            asset_family_id: FAMILY,
            asset_lock_csv_delta: it.asset_lock_csv_delta,
            payment_hash: it.payment_hash,
            lineage_proof_hash: [0x99; 32],
            lineage_verified_by: vec![[0xaa; 32]],
            anchor_inclusion_height: 840_001,
        }
    }

    /// genesis → register → commit → finalize → lock → finalize, driven by the node.
    #[test]
    fn full_mint_then_redeem_lifecycle() {
        let mut node = StateNode::genesis(FAMILY, [0x02; 32], 3, PRICE_50K);

        node.issuer_register(issuer()).expect("register");
        assert_eq!(node.state().state_epoch, 1);

        node.mint_commit(ISSUER_ID, &commit_witness())
            .expect("commit");
        assert_eq!(node.state().reserve_btc_sats, 4_000_000_000);
        assert_eq!(node.state().sat_usd_supply_atoms, 0); // no supply yet
        assert!(node
            .issuer(&ISSUER_ID)
            .unwrap()
            .pending_mint_commitment
            .is_some());

        let fin = mint::MintFinalizeWitness {
            issuer_id: ISSUER_ID,
            requested_mint_atoms: 100_000_000,
            deposit_txid: DEPOSIT_TXID,
            asset_metadata_commitment: META,
            mint_anchor_confirmations: 6,
            mint_proof_ok: true,
            oracle_price_e8: PRICE_50K,
        };
        node.mint_finalize(ISSUER_ID, &fin).expect("finalize");
        assert_eq!(node.state().sat_usd_supply_atoms, 100_000_000);
        assert!(node
            .issuer(&ISSUER_ID)
            .unwrap()
            .pending_mint_commitment
            .is_none());

        // Redeem: lock the SatUSD, then finalize against a confirmed BTC payout.
        let it = redeem_intent();
        let lr = lock_record(&it);
        node.redeem_lock(redeem::RedeemLockWitness {
            redeem_intent: it.clone(),
            lock_record: lr.clone(),
            lock_exclusion_proof: vec![],
            lineage_ok: true,
            lineage_proof_hash: [0x99; 32],
        })
        .expect("lock");

        let fin_w = redeem::RedeemFinalizeWitness {
            redeem_intent: it.clone(),
            lock_record: lr.clone(),
            lock_finalize: LockFinalizeRecord {
                lock_record_hash: lock_record_hash(&lr),
                payment_preimage: PREIMAGE,
                finalize_anchor_txid: [0x50; 32],
                finalize_anchor_outpoint: OutPoint {
                    txid: [0x50; 32],
                    vout: 0,
                },
                protocol_sink_script_key: satusd_crypto::nums::protocol_sink_script_key(&FAMILY),
                protocol_burn_internal_key: satusd_crypto::nums::protocol_burn_internal_key(
                    &FAMILY,
                ),
                finalized_amount_atoms: lr.lock_amount_atoms,
                operator_id: OPERATOR,
                finalize_height: 840_002,
                universe_burn_proof_hash: [0x60; 32],
            },
            btc_htlc: BtcHtlcPayoutRecord {
                operator_id: OPERATOR,
                redeem_intent_hash: redeem_intent_hash(&it),
                btc_htlc_txid: [0x70; 32],
                btc_htlc_vout: 0,
                payment_hash: it.payment_hash,
                user_claim_pubkey: it.user_btc_claim_pubkey,
                operator_refund_pubkey: [0x71; 32],
                payout_sats: 1_990_000_000,
                btc_csv_delta: it.btc_htlc_csv_delta,
                htlc_inclusion_height: 840_000,
                htlc_inclusion_block_hash: [0x72; 32],
                claim_spend_txid: [0x73; 32],
                revealed_preimage: PREIMAGE,
                claim_inclusion_height: 840_000,
                claim_inclusion_block_hash: [0x74; 32],
                confirmation_depth: 6,
            },
            operator_id: OPERATOR,
            price_e8: PRICE_50K,
            lock_membership_proof: vec![],
            consumed_exclusion_proof: vec![],
            refund_exclusion_proof: vec![],
            nullifier_exclusion_proof: vec![],
            burn_proof_ok: true,
        };
        node.redeem_finalize(fin_w).expect("redeem finalize");

        assert_eq!(node.state().sat_usd_supply_atoms, 0); // burned
        assert_eq!(node.state().reserve_btc_sats, 2_000_000_000); // 40 - 20 BTC
        assert_eq!(node.state().state_epoch, 5);
    }

    #[test]
    fn double_finalize_rejected_by_node() {
        // A second redeem_finalize of the same lock fails (lock now consumed).
        let mut node = StateNode::genesis(FAMILY, [0x02; 32], 3, PRICE_50K);
        node.issuer_register(issuer()).unwrap();
        node.mint_commit(ISSUER_ID, &commit_witness()).unwrap();
        let fin = mint::MintFinalizeWitness {
            issuer_id: ISSUER_ID,
            requested_mint_atoms: 100_000_000,
            deposit_txid: DEPOSIT_TXID,
            asset_metadata_commitment: META,
            mint_anchor_confirmations: 6,
            mint_proof_ok: true,
            oracle_price_e8: PRICE_50K,
        };
        node.mint_finalize(ISSUER_ID, &fin).unwrap();

        let it = redeem_intent();
        let lr = lock_record(&it);
        node.redeem_lock(redeem::RedeemLockWitness {
            redeem_intent: it.clone(),
            lock_record: lr.clone(),
            lock_exclusion_proof: vec![],
            lineage_ok: true,
            lineage_proof_hash: [0x99; 32],
        })
        .unwrap();

        let mk_fin = || redeem::RedeemFinalizeWitness {
            redeem_intent: it.clone(),
            lock_record: lr.clone(),
            lock_finalize: LockFinalizeRecord {
                lock_record_hash: lock_record_hash(&lr),
                payment_preimage: PREIMAGE,
                finalize_anchor_txid: [0x50; 32],
                finalize_anchor_outpoint: OutPoint {
                    txid: [0x50; 32],
                    vout: 0,
                },
                protocol_sink_script_key: satusd_crypto::nums::protocol_sink_script_key(&FAMILY),
                protocol_burn_internal_key: satusd_crypto::nums::protocol_burn_internal_key(
                    &FAMILY,
                ),
                finalized_amount_atoms: lr.lock_amount_atoms,
                operator_id: OPERATOR,
                finalize_height: 840_002,
                universe_burn_proof_hash: [0x60; 32],
            },
            btc_htlc: BtcHtlcPayoutRecord {
                operator_id: OPERATOR,
                redeem_intent_hash: redeem_intent_hash(&it),
                btc_htlc_txid: [0x70; 32],
                btc_htlc_vout: 0,
                payment_hash: it.payment_hash,
                user_claim_pubkey: it.user_btc_claim_pubkey,
                operator_refund_pubkey: [0x71; 32],
                payout_sats: 1_990_000_000,
                btc_csv_delta: it.btc_htlc_csv_delta,
                htlc_inclusion_height: 840_000,
                htlc_inclusion_block_hash: [0x72; 32],
                claim_spend_txid: [0x73; 32],
                revealed_preimage: PREIMAGE,
                claim_inclusion_height: 840_000,
                claim_inclusion_block_hash: [0x74; 32],
                confirmation_depth: 6,
            },
            operator_id: OPERATOR,
            price_e8: PRICE_50K,
            lock_membership_proof: vec![],
            consumed_exclusion_proof: vec![],
            refund_exclusion_proof: vec![],
            nullifier_exclusion_proof: vec![],
            burn_proof_ok: true,
        };
        node.redeem_finalize(mk_fin()).expect("first finalize");
        assert_eq!(
            node.redeem_finalize(mk_fin()),
            Err(NodeError::Redeem(redeem::RedeemRejectReason::LockConsumed))
        );
    }
}
