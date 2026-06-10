# ADR-0022: Two-phase ReserveClaim lifecycle + claim_id fixpoint fix

- **Date**: 2026-05-25
- **Status**: accepted
- **PRD reference**: §5.D12 (stale-claim reclaim), §5.D13 (claim_id), §5.D8 (CR), §6.8
- **Author**: Jeffery (with implementation from AI coding agent)
- **Amends**: [ADR-0012](ADR-0012-claim-id-preimage.md)

## Context

Wiring the §5.D12 two-phase claim lifecycle into the state node surfaced two
coupled problems.

**1. `REDEEM_FAST_FINALIZE` accounting was collapsed.** The single-redemption
`redeem.rs` finalize and the batched `claim.rs` verifier both debited
`reserve_btc_sats` directly and never touched `reserved_pending_claim_sats` /
`pending_claim_root`. But §5.D12 specifies two phases:

```
submit_claim:   reserved_pending_claim_sats += reimbursement   (no reserve debit)
finalize_claim: reserved_pending_claim_sats -= reimbursement, reserve_btc_sats -= reimbursement
RECLAIM:        reserved_pending_claim_sats -= reimbursement, status → RECLAIMED
```

Without the reserve step, `pending_claim_root` was never populated and the
`RECLAIM_STALE_CLAIM` verifier had nothing to free.

**2. `claim_id` is self-referential.** `claim_id` (§5.D13) keys the `PendingClaim`
inserted into `pending_claim_root`, which is part of `new_state_root`. But
`encode_for_claim_id` included `new_state_root`, so
`claim_id → new_state_root → pending_claim_root → claim_id` — an unsatisfiable
fixpoint at submit time.

## Decision

**Two-phase submit.** `REDEEM_FAST_FINALIZE` (`claim::verify_reserve_claim_finalize`)
is the **submit_claim** phase: it burns the redeemed supply, consumes the locks +
nullifiers, sets `reserved_pending_claim_sats += reimbursement`, and inserts a
PENDING `PendingClaim` keyed by `claim_id` — it does **not** debit
`reserve_btc_sats`. CR/tier are computed on the **full** `reserve_btc_sats`
(§5.D8 formula, consistent with mint), so CR rises at submit (supply drops) and
falls back when the reserve is later debited at finalize.

**finalize_claim** is a new transition `FINALIZE_CLAIM = 0x31` (next to
`RECLAIM_STALE_CLAIM = 0x30`; distinct from the Tier-3 `Settle = 0x70` stub). It
pays out an approved PENDING claim: `reserved_pending_claim_sats -= reserved`,
`reserve_btc_sats -= reserved`, CR/tier recomputed on the reduced reserve, PENDING
→ FINALIZED. It must run before `claim_expiry_height` (after expiry it is RECLAIM
territory). The reserve backend-approval signature is a deferred fact (MVP
MultisigReserve).

**`claim_id` excludes `new_state_root`** (amends ADR-0012 / §5.D13). The preimage
now binds the claim's *inputs* only: `transition_type`, `operator_id`,
`prev_state_root`, all batch roots, `l1_anchor`, `reserve_shard_id`,
`reimbursement_sats`, `proof_commitment`, `claim_expiry_height` — not
`new_state_root`, `claim_id`, or `operator_signature`. The post-state is
deterministic from those inputs + `prev`, so it stays implicitly bound. The full
`ReserveClaim` canonical `Encode` still contains `new_state_root` (placed right
after `claim_id`); only the `claim_id` preimage drops it.

## Consequences

- `encode_for_claim_id` (Rust `types.rs` + TS `satusd.ts`) drop `new_state_root`;
  the full `Encode` reorders it after `claim_id`. Vectors regenerated; Rust = TS.
- `claim.rs` submit reserves + creates the PENDING claim; new reject reasons
  `ClaimIdMismatch`, `DuplicateClaim`. The `DuplicateClaim` branch is unreachable
  in isolation (claim_id binds `prev_state_root`), kept as the SMT-insert
  precondition; idempotency is exercised at the node level.
- `StateNode` gains `submit_claim` (returns the `claim_id` handle) and
  `reclaim_stale_claim`, plus a `pending_claims` map and `set_l1_anchor` (L1
  follower sync for ClaimClock freshness).
- The legacy collapsed-model `redeem::redeem_finalize` (fact-based confirmation,
  used by the HTTP demo) is left in place for now; it should converge to the
  two-phase path.

## Follow-ups

- ~~Assign a `TransitionType` to finalize_claim and implement the reserve debit +
  PENDING → FINALIZED transition.~~ Done: `FINALIZE_CLAIM = 0x31`,
  `registry::verify_finalize_claim`, `StateNode::finalize_claim`.
- ~~Model the reserve backend-approval signature that authorizes finalize.~~ Done
  in [ADR-0023](ADR-0023-reserve-committee-approval.md): a committed 3-of-5
  reserve-committee multisig over `claim_id`.
- ~~Converge `redeem::redeem_finalize` onto the two-phase model (or retire it in
  favor of the N=1 batched claim).~~ Done: the single-redemption finalize (and its
  HTTP `redeem_finalize` endpoint) were retired; the batched `claim.rs` (N≥1) is
  the canonical REDEEM_FAST_FINALIZE. A `submit_claim` HTTP endpoint (oracle
  messages + SPV + ClaimClock DTOs) is a follow-up.
