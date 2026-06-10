# ADR-0019: IssuerPosition gains `pending_mint_commitment` (two-phase mint linkage)

- **Date**: 2026-05-24
- **Status**: accepted
- **PRD reference**: §5.D11 (issuer lifecycle), §8.1 (software verifier), §13.3 (I-03, I-07)
- **Author**: Jeffery (with implementation from AI coding agent)

## Context

M2's MINT transition engine needs MINT_FINALIZE to (a) match the asset
metadata/anchor committed at MINT_COMMIT (I-03) and (b) reject a second finalize
of the same commit (I-07). The frozen §6.1 `StateRoot` / §5.D11 `IssuerPosition`
tracked pending mint only as an aggregate `pending_mint_atoms` counter — there is
no per-commit record of *what* was committed, so neither test could be enforced.

## Options Considered

1. **Add `pending_mint_commitment: Option<[u8;32]>` to `IssuerPosition`** (chosen).
   One outstanding mint per issuer: COMMIT requires it be `None` and sets it to
   `mint_commitment(requested_atoms, asset_metadata_commitment, deposit_txid)`;
   FINALIZE matches the witness against it and clears it. Minimal extension;
   passes all of I-01..I-07.
2. Add a dedicated `pending_mint_root` SMT to `StateRoot`, keyed by
   `mint_request_hash`. Supports multiple concurrent pending mints per issuer;
   more general but extends the top-level state + adds SMT wiring + complexity.
3. Defer I-03/I-07 to a later chunk (no struct change now).

## Decision

Adopt Option 1. `IssuerPosition` gains a trailing
`pending_mint_commitment: Option<[u8;32]>` field, encoded last via the existing
`opt` convention (§18.1). `mint_commitment` is a new derivation under the
existing `SATUSD_MINT_REQUEST_V1` domain (§18.2):
`tagged_hash(SATUSD_MINT_REQUEST_V1, u64(requested_atoms) || asset_metadata_commitment || deposit_txid)`.

**Constraint**: at most one pending mint per issuer (COMMIT rejected with
`PendingMintExists` otherwise). Acceptable for MVP; revisit (Option 2) if
concurrent per-issuer mints are needed.

## Consequences

- `IssuerPosition` canonical encoding + `issuer_position_hash` change → this is a
  v5.2 struct revision; test vectors regenerated (Rust/TS still byte-match).
- Enables the §8.1 MINT software verifiers (`satusd-state::mint`) to pass
  I-01..I-07, including DL-27 (I-04: finalize-time CR re-check).
- PRD §5.D11 `IssuerPosition` struct updated to include the field.

## References

- `crates/satusd-state/src/mint.rs`, `satusd_types::derive::mint_commitment`
- PRD §5.D11, §13.3 (I-03, I-07)
