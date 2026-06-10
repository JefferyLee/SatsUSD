# ADR-0023: Reserve-committee approval gating FINALIZE_CLAIM

- **Date**: 2026-05-25
- **Status**: accepted
- **PRD reference**: §4 (Reserve Committee role), §11.2 (MultisigReserve), §5.D12
- **Author**: Jeffery (with implementation from AI coding agent)
- **Follows**: [ADR-0022](ADR-0022-two-phase-claim-and-claimid-fixpoint.md)

## Context

ADR-0022 added `FINALIZE_CLAIM = 0x31` (pay out an approved PENDING claim: debit
`reserve_btc_sats`, free the reservation, PENDING → FINALIZED) but left the
*authorization* as a deferred fact — any caller could finalize. Per §4 and §11.2,
a claim is only paid out once the **Reserve Committee (3-of-5 multisig, distinct
from issuers)** approves; MultisigReserve broadcasts the reserve spend when the
3-of-5 is reached. So finalize must require a committee approval, and the committee
must be **state-committed** — otherwise an attacker supplies their own committee.

## Decision

Add a `reserve_committee_hash: [u8; 32]` field to `StateRoot`, committing the
active committee config exactly as `oracle_set_hash` commits the oracle signer set:

```
reserve_committee_hash = SHA256("SATUSD_RESERVE_COMMITTEE_V1"
                                || u8(threshold) || sorted(committee_pubkeys[33]))
```

`FINALIZE_CLAIM` (`registry::verify_finalize_claim`) now takes a `ReserveCommittee`
`{ threshold, pubkeys }` and `approvals: &[MultisigSig]`, and requires:
- `reserve_committee_hash(threshold, pubkeys) == prev.reserve_committee_hash`
  (`CommitteeMismatch`);
- ≥ `threshold` distinct, valid secp256k1 ECDSA signatures over `claim_id`
  (`ApprovalInsufficient`), reusing the issuer-multisig signature primitive.

The committee config is governance state (set at genesis / rotated via GOVERN); the
node configures it with `StateNode::set_reserve_committee` and supplies the
committee + approvals to `finalize_claim`. Like the existing `lock_script_key`
derivation, the secp256k1 verification is software/challenger-enforced off-circuit
(DL-25), so no circuit or TS mirror is required — but the new `StateRoot` field and
domain are in the 3-way canonical-encoding vectors.

## Consequences

- New `StateRoot` field `reserve_committee_hash` (after the oracle config cluster)
  in Rust + TS `encodeStateRoot`; new domain `SATUSD_RESERVE_COMMITTEE_V1` (27th);
  vectors regenerated (Rust = TS byte-match).
- New derive `reserve_committee_hash`; new `registry::ReserveCommittee`; new reject
  reasons `CommitteeMismatch` / `ApprovalInsufficient`.
- `genesis` leaves `reserve_committee_hash = [0; 32]` (unconfigured); a committee
  must be set before any finalize succeeds.
- Tests: `finalize_insufficient_approval` (2-of-5 → reject), `finalize_wrong_committee`
  (uncommitted committee → reject); the node submit→finalize lifecycle now signs
  3-of-5 over the returned `claim_id`.

## Follow-ups

- Committee rotation via GOVERN (0x60) is still a stub.
