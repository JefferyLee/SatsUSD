# ADR-0016: Status enum discriminants and the FreezeReason variant set

- **Date**: 2026-05-24
- **Status**: accepted
- **PRD reference**: §5.D11, §5.D12, §18.2
- **Author**: Jeffery (with implementation from AI coding agent)

## Context

`IssuerPosition` (§5.D11) and `PendingClaim` (§5.D12) carry status enums whose
numeric discriminants the PRD never fixes, and `IssuerPosition.freeze_reason` is
typed `Option<FreezeReason>` but the PRD never enumerates `FreezeReason` — it only
lists five freeze triggers in §5.D11 prose. These discriminants enter the
canonical encoding (and the `issuer_positions_root` / `pending_claim_root` SMTs),
so they are permanent protocol constants that must be frozen in M0.

## Decision

Discriminants are **0-based in declaration order**:

- `IssuerStatus`: `Active=0, Paused=1, Frozen=2, Exiting=3`.
- `PendingClaimStatus`: `Pending=0, Finalized=1, Challenged=2, Expired=3, Reclaimed=4`.
- `FreezeReason` mirrors the five §5.D11 triggers:
  `OracleUnavailable=0, IndividualCrBelowTier1=1, ReserveCommitteeVote=2,
  MintProofMismatch=3, ManualEmergencyPause=4`.

Encoding: each status is a single `u8`; `freeze_reason` is `Option<u8>` (presence
flag + value); both reference the registered domain separators
`SATUSD_ISSUER_POSITION_V1` / `SATUSD_PENDING_CLAIM_V1` for their record hashes.

Considered and rejected: 1-based numbering reserving `0` as an `Invalid` guard —
adds a variant and forbids `status == 0`, with no benefit given encodings always
construct a concrete variant.

## Consequences

- Positive: `IssuerPosition` / `PendingClaim` are now fully specified, encoded,
  hashed, and cross-language byte-matched (vectors + assertions extended).
- Trade-off: 0-based means a zero byte is a valid status (`Active` / `Pending`);
  callers must construct explicit variants rather than rely on zero-init.
- The `FreezeReason` set is now protocol surface; new triggers require a new
  variant (append-only, never renumber) and an ADR.

## References

- `crates/satusd-types/src/types.rs` (`IssuerStatus`, `FreezeReason`,
  `PendingClaimStatus`, `IssuerPosition`, `PendingClaim`)
- `crates/satusd-types/src/derive.rs` (`issuer_position_hash`, `pending_claim_hash`)
- `integration/vectors/vectors.json` (`IssuerPosition`, `PendingClaim` vectors)
