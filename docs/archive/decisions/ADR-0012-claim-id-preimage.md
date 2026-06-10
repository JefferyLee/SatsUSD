# ADR-0012: `claim_id` preimage excludes `claim_id` and `operator_signature`

- **Date**: 2026-05-23
- **Status**: accepted
- **PRD reference**: §5.D13, §6.8
- **Author**: Jeffery (with implementation from AI coding agent)

## Context

§5.D13 defines `claim_id = SHA256("SATUSD_CLAIM_ID_V1" || canonical_encode(ReserveClaim_without_signatures))`.
But `claim_id` is itself the first field of `ReserveClaim` (§6.8). A field cannot be part of
its own preimage, so "without signatures" alone is under-specified.

## Options Considered

1. **Exclude both `claim_id` and `operator_signature`** from the preimage.
   - Pros: only self-consistent reading; `claim_id` is a stable function of the claim body,
     unaffected by who/whether it is signed; matches the §5.D19 idempotency intent
     (same logical claim → same id).
   - Cons: "without signatures" in the PRD does not literally mention `claim_id`.
2. Exclude only `operator_signature`, include a zeroed `claim_id`.
   - Pros: literal-ish reading.
   - Cons: requires a zeroing convention the PRD never states; brittle and surprising.

## Decision

Adopt **Option 1**. `ReserveClaim::encode_for_claim_id` encodes all fields in declaration
order except `claim_id` and `operator_signature`; `derive::claim_id` hashes the domain tag
followed by that encoding. A unit test asserts `claim_id` is invariant under changes to
`claim_id`/`operator_signature` and sensitive to body-field changes.

## Consequences

- Positive: idempotent claim identity independent of signature material.
- Follow-up: clarify §5.D13 to say "excluding `claim_id` and `operator_signature`".

## References

- `crates/satusd-types/src/types.rs::ReserveClaim::encode_for_claim_id`
- `crates/satusd-types/src/derive.rs::claim_id`
- `crates/satusd-types/tests/encoding.rs::claim_id_ignores_claim_id_and_signature_fields`
