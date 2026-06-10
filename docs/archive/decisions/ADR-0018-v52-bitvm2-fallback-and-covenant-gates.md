# ADR-0018: v5.2 — BitVM2 fallback, advisory gates G5/G6, covenant risk

- **Date**: 2026-05-24
- **Status**: accepted
- **PRD reference**: §0 (DL-33, DL-34), §5.D9, §11.5, §16.4 (G5/G6), §17 (R-coordination-1), §22
- **Author**: Jeffery (proposal) + AI agent (review + edit)

## Context

Two risk-management additions were proposed for the PRD: (a) a BitVM2 reserve as
a fallback if BitVM3 upstream isn't mature enough by M7; (b) periodic tracking of
the Bitcoin covenant soft-fork landscape, which affects BitVM cost/feasibility.
Both fit the PRD's progressive-decentralization and discovery-gate philosophy and
the §5.D9 two-layer backend abstraction. The draft text had factual/consistency
issues that this ADR's edits correct.

## Decision

Promote to **v5.2** and add:

- **§11.5 BitVM2Reserve as Fallback** (DL-33). Implements the same §5.D9
  `OptimisticEnforcementBackend` as BitVM3Reserve (user-transparent); added a
  BitVM2Reserve row to the §5.D9 backend table. Migration to BitVM3 remains open
  post-M8 (same trait).
- **§16.4 advisory gates G5/G6** (DL-34), explicitly **non-blocking** and distinct
  from the hard gates G1–G4: G5 Covenant Landscape Review (every 6 months), G6
  BitVM upstream readiness (M6, feeds the M8 BitVM2-vs-BitVM3 choice).
- **§17 R-coordination-1** (P1): covenant activation-path uncertainty.

Corrections applied to the original proposal during review:

1. **BIP numbers** fixed to the verified set — **119 (CTV), 347 (OP_CAT), 348
   (CSFS), 345 (OP_VAULT), 118 (APO), 420 (OP_CAT bundle)**. The draft's
   BIP-443/446/448 are not real covenant BIPs.
2. **R-coordination-1 reframed**: BitVM2 runs on *current* Bitcoin script (no
   soft-fork needed) — the real risk is **cost**, i.e. OP_CAT (BIP-347, a major
   BitVM cost reduction) stalling while only CTV (BIP-119, limited BitVM benefit)
   activates — not loss of feasibility.
3. **§11.5 cross-reference** points to **G6** (readiness assessment), not G4.
4. Order-of-magnitude figures (~10–100 GB / ~1–10 GB / ~1000×) labeled estimates,
   "as of 2026, re-check at G6".
5. G5/G6 framed as advisory/periodic, preserving §16.4's "four hard gates".
6. §5.D9 backend table updated so BitVM2Reserve traces as a contract entry.

## Consequences

- Positive: a verified fallback path de-risks M8; covenant risk is tracked
  without betting the architecture on any single BIP.
- These are accepted v5.2 additions (per §22 change control: DL entries + this
  ADR). External protocol-reviewer sign-off still recommended at the next gate.

## References

- PRD §11.5, §16.4 (G5/G6), §17 R-coordination-1, DL-33/34, §22 v5.2 row
- BIP verification: bitcoinops.org / Bitcoin Magazine (CTV 119, OP_CAT 347, CSFS 348, OP_VAULT 345, APO 118)
