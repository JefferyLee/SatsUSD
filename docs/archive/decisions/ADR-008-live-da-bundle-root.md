# ADR-008: Live DA bundle format + the `live_da_root` definition (M5)

- **Date**: 2026-05-25
- **Status**: accepted
- **PRD reference**: §10.1 / §10.3 (DA layer), §6.1 (StateRoot.live_da_root),
  §5.D12 (ReserveClaim), §13.4 (DA-01..06), §14.12 (M5). Relates to ADR-0011
  (domain-separator / canonical encoding).
- **Author**: Jeffery (with implementation from AI coding agent)

## Context

M5's challenger must, given a `submit_claim`, fetch the operator's Live DA bundle
and independently recompute the claim (lineage / SPV / software verifier). The
bundle's root is committed on-chain in the `ReserveClaim` as `live_da_root`, so its
definition is consensus-relevant.

The PRD gives **two non-equivalent definitions** of `live_da_root`:

- §10.1: `live_da_root = SHA-256(canonical-encode(LiveDABundle))`.
- §10.3: `live_da_root = SHA256(concat(SHA256(section_i) for each section))`.

These disagree. A flat hash of the whole bundle (§10.1) cannot, on its own, let a
challenger attribute a mismatch to a *specific missing/altered section* — yet
DA-03 (missing oracle messages, section 0x08) and DA-06 (missing TA lineage,
section 0x03) require exactly that structural detection.

## Decision

1. **Adopt the §10.3 per-section root.** `live_da_root =
   SHA256( concat( SHA256(section_wire_i) for each section in ascending id ) )`,
   where `section_wire_i = id (u8) || size (u32 big-endian) || content`. Integers
   are big-endian, matching the §18.1 canonical encoding (ADR-0011). Implemented in
   `crates/satusd-da` (`LiveDABundle::live_da_root`), cross-language pinned by the
   `live_da_root` vector (Rust = TypeScript `liveDaRootHex`).

2. **§10.1's phrasing is superseded** and should be corrected in the PRD to the
   §10.3 form (same treatment as the §18.1 "zero-pad to 32 bytes" note in
   `domain.rs`). The flat-encode form is kept only as an informal description of
   "a hash over the bundle", not the normative root.

3. **The bundle header carries the root for transport, but it is not trusted.**
   `decode` returns the header's declared root *and* the parsed sections; the
   challenger compares the **claim's committed** `live_da_root` against the
   **recomputed** root — a tampered header or body surfaces as a mismatch (DA-02),
   not a parse success.

## Consequences

- A challenger can hash sections independently, so a dropped section (DA-03 /
  DA-06) changes the root and is detectable without trusting the operator's header.
- `live_da_root` is now a real, reproducible commitment; wiring it into
  `submit_claim` (replacing the `[0x0a; 32]` placeholder) is the next M5 step.
- The section-id space and ordering (0x01..0x0E, ascending) are frozen by §10.3;
  adding a section is a versioned change (bundle `version` byte).

## Alternatives considered

- *Flat `SHA256(canonical-encode(bundle))` (§10.1).* Rejected: no per-section
  attribution, so DA-03/DA-06 detection degrades to "something differs".
- *A Merkle tree over sections.* Rejected for the MVP: with ≤ 14 fixed sections the
  flat concat-of-hashes is simpler and the inclusion-proof benefit is unused (the
  challenger always fetches the whole bundle).
