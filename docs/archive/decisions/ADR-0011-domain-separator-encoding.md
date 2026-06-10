# ADR-0011: Canonical hash domain-separator convention

- **Date**: 2026-05-23
- **Status**: accepted
- **PRD reference**: §18.1, §18.2, §5.D1, §5.D3, §6.2, §5.D13
- **Author**: Jeffery (with implementation from AI coding agent)

## Context

The PRD is internally contradictory about how a domain separator is mixed into a
domain-tagged hash:

- **§18.1** gives the only fully worked example (`redeem_intent_hash`) and states the
  domain is the ASCII name **zero-padded to 32 bytes**: `SHA256(domain_32 || body)`.
- **§5.D1, §5.D3, §6.2, §5.D13** all write the domain as a **raw ASCII prefix** with no
  padding: `SHA256("SATUSD_..._V1" || body)`.

These produce different bytes for every hash. The two conventions cannot be reconciled
because **§18.2 registers `SATUSD_BTC_PAYOUT_CONFIRMATION_V1`, which is 33 bytes** — it
does not fit in a 32-byte field at all. The 33-byte registered name is decisive evidence
that universal 32-byte padding was never actually intended.

This must be frozen in M0 because it determines the bytes of every test vector.

## Options Considered

1. **Raw ASCII prefix, no padding** — `SHA256(name_bytes || body)`.
   - Pros: matches 4 of 5 spec sites; works for names of any length, including the 33-byte
     one; simplest to implement and reason about.
   - Cons: contradicts §18.1's literal "32 bytes" wording (which is itself already broken
     by the 33-byte registered name).
2. **Fixed 64-byte zero-pad** — keep a fixed-width tag but widen to 64B.
   - Pros: preserves §18.1's fixed-width intent; fits all current names.
   - Cons: diverges from both the literal "32B" and the raw-prefix style used everywhere
     else; an arbitrary width with no spec basis.
3. **Keep 32B, rename the long separator** to fit.
   - Pros: honors §18.1 literally.
   - Cons: mutates the §18.2 registry; renaming a security-relevant domain tag is itself a
     spec change with no upside over option 1.

## Decision

Adopt **Option 1: raw ASCII prefix, no padding**. Every domain-tagged hash is
`SHA256(name_ascii_bytes || body)`. Implemented centrally in
`crates/satusd-types/src/domain.rs::domain_tag` and mirrored in the TypeScript reference.

§18.1 must be corrected to drop "zero-padded to 32 bytes" and state the raw-prefix rule.

## Consequences

- Positive: one consistent rule across all derivations; no length constraint on separators;
  Rust and TypeScript references byte-match on 1043 vectors.
- Trade-off: the PRD §18.1 text is now authoritative-by-ADR rather than as-written; the
  PRD edit is still pending.
- Follow-up: edit PRD §18.1; add a regression vector specifically asserting the
  no-padding behavior for the 33-byte separator.

## References

- `crates/satusd-types/src/{domain.rs,hash.rs,derive.rs}`
- `integration/vectors/vectors.json` (`domain_padding: "raw-ascii-no-pad"`)
- Related task: satusd-types foundation (M0 W1–2)
