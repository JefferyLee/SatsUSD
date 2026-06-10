# ADR-0001: Restart the project from MISSION v2

- **Date**: 2026-06-09
- **Status**: accepted
- **Authority**: `docs/MISSION.md` (v2) — this ADR implements its
  closing clause: implementation must derive from the mission.
- **Author**: Jeffery (decisions) + AI agent (drafting)

## Context

The project went through multiple directions during the v5.1 era:
some proved technically blocked (in-circuit SHA-256/secp256k1,
BitVM3 upstream dependence), some worked but drifted from the
original intent (a committee-gated reserve architecture that
accumulated trust assumptions the mission exists to remove; a
single protocol-chosen oracle pipeline).

The original seed vision was then written down and refined into
`docs/MISSION.md` v2. Its key commitments that the v5.1
architecture does not serve:

1. **The mission is to migrate the price-signal function** of money
   from fiat to bitcoin — not merely to operate a stablecoin.
2. **Self-referencing is the definition of success**: the canonical
   price ultimately derives from SatUSD's own on-chain market
   (information self-reference on exogenous BTC collateral —
   categorically distinct from UST's collateral circularity).
3. **Trust is priced by a market, not chosen by decree**: redemption
   runs over an open standard of competing rails (oracle classes
   with different speed/size/fee/trust profiles); the
   self-referencing rail wins by competition, not by governance
   switch.
4. **Liveness is bought, not assumed**; everything is verifiable by
   anyone; founder, scaffolding, and eventually the asset itself all
   exit ("three exits, one philosophy").

Strategy decisions already locked (2026-06-09): DLC redemption from
day one; initial reserve self-funded by the founder; no legal
entity; no governance token; engineering on an AI-assisted (~10x)
clock, adoption on the world's clock.

## Decision

1. **`docs/MISSION.md` v2 is the root authority.** All other
   documents derive from it. The v5.1 PRD, ADR-001..0023, discovery
   reports, M6 runbooks, and the old `SECURITY.md` /
   `ARCHITECTURE.md` move to `docs/archive/` and are superseded in
   their entirety. The ADR series restarts with this document.

2. **The new specification lives in `docs/spec/`** (replacing the
   PRD form):

   | File | Owns |
   |---|---|
   | `00-overview.md` | System map + terminology |
   | `01-asset.md` | TA asset, burn-to-NUMS, supply commitments |
   | `02-rail-standard.md` | **Core**: quote/lock/settle/dispute state machine + RailManifest (self-certifying rail IDs) |
   | `03-oracle-classes.md` | Oracle class registry, dlcspecs compatibility, graduation by client defaults |
   | `04-reserve.md` | Per-rail reserve structure, CR rules, auditability |
   | `05-dispute.md` | BitVM disprove hooks, tlock liveness escapes, liveness economics |

3. **Code salvage map.** Existing crates remain in the workspace and
   building until replaced; each is judged by the mission:

   | Asset | Verdict | Reason |
   |---|---|---|
   | `satusd-crypto` | **keep** | Mission-neutral primitives (Poseidon/SMT/NUMS/signatures); cross-language parity is a hard-won asset |
   | `satusd-ta-proof` | **keep** | Core of "everything verifiable by anyone" |
   | `satusd-types` | keep frame, re-audit fields | Encoding discipline survives; structures will change with the rail standard |
   | `ts/` + vectors + `make verify` | **keep** | Machine-verifiable correctness is the answer to single-human review bandwidth |
   | `devnet/` | keep | Infrastructure |
   | `satusd-state` | **rewrite** | Shaped around a single committee-gated reserve; the rail standard dictates a different shape |
   | `satusd-reserve` | demote | 3-of-5 multisig becomes one rail's backend option, not THE reserve |
   | `satusd-dispute` | generalize | Becomes the rail standard's dispute hook |
   | `circuits/` | hold | Bound to the old state commitment; re-judge after the new spec settles |
   | `satusd-operator` / `-challenger` / `-state-node` | rewrite | Built around the old architecture |

4. **Build order** (engineering clock): `02-rail-standard.md` first;
   then `01`/`03`; then Rail-0 (RFQ atomic swap — the degenerate
   no-oracle class) and Rail-1 (single-oracle DLC) reference
   implementations; then the oracle daemon (dlcspecs format,
   explicitly transitional); then signet public alpha. Adoption
   gates thereafter are metric-based, not calendar-based.

5. **Naming**: the canonical asset name is **SatUSD** (singular unit
   grammar, consistent with `sat`). The repository directory name
   `SatsUSD` is retained for continuity.

## Consequences

- The repo carries its own history: the archive records what was
  tried and why it was left behind ("what we learn will be shared").
- `make check` stays green throughout — old crates keep building
  until their replacements land, so the cross-language assertion
  suite continues to guard the kept primitives.
- `docs/WORKFLOWS.md` references archived paths and needs a
  revision pass once `docs/spec/` lands.
- `docs/proposals/0001-*` (TA-in-DLC funding output + PSBT appendix)
  remain live but their oracle assumptions must be revised to the
  rail/oracle-class model.
- A new `SECURITY.md` (trust model of the rail architecture) and
  `ARCHITECTURE.md` will be written after the spec settles; until
  then the archived versions describe only the past.
