# ADR-0021: OperatorPosition struct + OPERATOR_REGISTER

- **Date**: 2026-05-24
- **Status**: accepted
- **PRD reference**: §3 (operator role), §5.D10, §5.D12, §6, §18.3 (OPERATOR_BOND_MULTIPLE)
- **Author**: Jeffery (with implementation from AI coding agent)

## Context

OPERATOR_REGISTER (0x20) is an MVP-required transition (§5.D10), but `§6` defined
no `OperatorPosition` struct — only the `SATUSD_OPERATOR_POSITION_V1` domain was
registered. The operator role is otherwise well-specified: a whitelisted entity
that posts bond (`OPERATOR_BOND_MULTIPLE = 2`, bond ≥ 2× max single claim),
provides quotes, fronts BTC, batch-claims, signs `ReserveClaim`s, and whose bond
is partially slashed to a keeper on stale-claim reclaim (§5.D12).

## Decision

Define `OperatorPosition` (§6), stored in `operator_registry_root` (SMT keyed by
`operator_id`, leaf = `operator_position_hash` under
`SATUSD_OPERATOR_POSITION_V1`):

```rust
struct OperatorPosition {
  operator_id:            [u8; 32],
  status:                 OperatorStatus,  // Active=0 / Suspended=1 / Slashed=2
  operator_pubkey:        [u8; 33],        // secp256k1; signs ReserveClaims
  bond_sats:              u64,
  max_claim_sats:         u64,             // max single claim
  outstanding_claim_sats: u64,             // sum of pending claim reimbursements
  slashed_sats:           u64,             // cumulative slashed to keepers
  registered_at_height:   u32,
}
```

The documented bond/claim fields are included now (not speculative — §18.3 and
§5.D12 specify them) to avoid a second frozen-struct revision when the claim/
slash flow lands.

**OPERATOR_REGISTER** inserts a fresh operator: `status == Active`,
`outstanding_claim_sats == 0`, `slashed_sats == 0`, `max_claim_sats > 0`,
`bond_sats ≥ OPERATOR_BOND_MULTIPLE × max_claim_sats`, and the operator must be
absent from `operator_registry_root`.

## Consequences

- New §6 struct + `OperatorStatus` enum (0-based, ADR-0016 style); cross-language
  vectors added (`operator_position_*` + `operator_position_hash`); HTTP node gets
  an `operator_register` endpoint.
- **Deferred** (with the claim/reserve flow): the actual bond BTC deposit SPV at
  register time, `outstanding_claim_sats` accounting on claim submit, and bond
  slashing to keepers on RECLAIM_STALE_CLAIM (§5.D12). Status transitions
  (Active→Suspended/Slashed) are governance/claim-flow concerns.

## References

- `crates/satusd-state/src/registry.rs` (`apply_operator_register`),
  `satusd_types::types::OperatorPosition`, `derive::operator_position_hash`
