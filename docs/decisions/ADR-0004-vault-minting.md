# ADR-0004: Vault minting — mint-as-position-opening is the dismantling path for the founder-funded reserve

- **Date**: 2026-06-11
- **Status**: accepted (direction; M-C/M-D-era implementation —
  explicitly NOT on the M-B critical path, ADR only for now);
  **re-scoped by [ADR-0005](ADR-0005-no-transfer-redemption-notes.md)**
  (2026-06-16): the vault is a collateral primitive for **both** a CDP
  self-minter **and** an LP-issuer sourcing per-note `Q`; the
  reserve-coupled CDP flow + common-reserve clearing below are deferred
  to the covenant era (spec 04 scope note)
- **Authority**: `docs/MISSION.md` v2; PRD §8 scaffolding ledger;
  ADR-0002 (capacity-not-identity)
- **Author**: Jeffery (decisions) + AI agent (drafting)

## Context

Three converging pressures surfaced while building the public pitch
and the prior-art review:

1. **The junior tranche is one person.** SatUSD holders own the
   senior claim on the reserve; the junior side — the party
   absorbing BTC volatility — is the reserve's owner, i.e. the
   founder. There is no mechanism for anyone else to take junior
   exposure, so reserve size = founder capital = a hard supply
   ceiling. The scaffolding ledger's removal criterion for the
   founder-funded reserve ("external base large enough") named no
   mechanism by which that could happen.
2. **No entity, no token** (founding decisions) forbids
   securitizing junior exposure. Whatever distributes it must be a
   *protocol position*, not an instrument.
3. **The strongest prior-art objection** (10101 post-mortem, see
   cover post): "someone must fund the over-collateralized long
   side — who, and why?" A design where the question has no native
   answer inherits 10101's structural problem.

## Decision

Anyone can mint SatUSD by opening a **vault**: a fixed-maturity DLC
collateral position funded with their own BTC. 铸造即开仓.

Three-layer architecture — the asset and redemption paths do not
change:

```
mint side                clearing layer            redeem side
vaults (anyone)   ──→    common reserve     ──→    rails (spec 02)
lock own BTC,            receives face value       holders burn
mint SatUSD              from vault settlement     SatUSD for BTC
```

Vault mechanics (normative sketch; precision lands in a future
spec 06):

- **Open**: lock BTC in a DLC funding output (the proposal-0001
  construction) at or above the minimum opening CR (spec 04 §5
  parameter; conservative start, tuned by data). Mint face-value
  SatUSD. Pay a mint fee; the retained share sinks into the common
  reserve.
- **Checkpoint CETs instead of liquidation.** At open, pre-sign
  early-settlement CETs for each checkpoint (every N blocks) —
  **crash buckets only**. A healthy price at a checkpoint
  corresponds to no pre-signed transaction at all: nothing can be
  broadcast, the vault glides on. A price in a crash bucket (CR
  approaching the floor) makes that bucket's CET decryptable from
  the oracle attestation, and **anyone** may broadcast it for a
  bounty: face value + penalty to the common reserve, remainder
  back to the minter. Liquidation becomes an anyone-can-act market
  role, like challengers and broadcasters (spec 02 §5 tier 2).
- **Maturity**: the minter either proves the face-value burn and
  reclaims all collateral, or lets the maturity CET settle — face
  value to the reserve, excess back. Rolling is closing + reopening
  (an explicit roll fee, retained to the reserve — the carry cost
  exists, is priced, and accrues to the commons rather than to an
  exchange).
- **Pre-signed from birth**: a vault is Stage-2-grade (spec 04 §3)
  on day one — the construction *is* a one-shot pre-signing; there
  is no manual stage to retire.

## Consequences

1. **Scaffolding ledger** (PRD §8): the founder-funded reserve's
   removal criterion becomes mechanical — *vault-minted supply ≥
   50% of circulating SatUSD*. This ADR is the named mechanism the
   ledger previously lacked.
2. **System CR** (spec 04 §5) counts vault collateral in the
   numerator: crash-bucket CETs deliver face value to the reserve,
   so vault collateral is reserve-grade up to gap risk.
3. **Gap risk is conceded and priced ex-ante** — not solved.
   Checkpoint spacing + the opening CR bound the absorbable gap;
   beyond it, the loss lands on the pooled cushion and, past 100%
   system CR, on the NAV floor (spec 04 §5). This honesty is the
   position defended in the cover post against the
   liquidation-engine alternatives.
4. **Demand engine**: a vault is "spend dollars without selling
   bitcoin" — the HODLer's collateralized credit line, with no
   custodian and no bilateral lender. It answers the 10101
   objection natively: the leveraged-long side and the collateral
   source are the same actor with an existing, proven motive.
5. **Outside capital joins by using the mechanism** — opening
   vaults (junior + leverage) or staking as rail LPs (fees). No
   entity, no token ⇒ there is no equity to sell and none shall be
   implied. Any negotiated early arrangement goes on the
   scaffolding ledger with a removal criterion.
6. **Not now**: no vault prototype before M-B's adoption items
   complete. Spec 04 is written CDP-aware so the CR rules need not
   be re-derived when vaults land.

## Alternatives considered

- **Pooled CDP with liquidation auctions** (MakerDAO shape):
  unimplementable on Bitcoin L1 without a contract VM; sidechain
  versions import a bridge/federation (see cover post prior-art).
- **Margin-call via continuous oracle + bilateral channels**
  (10101 shape): dies with the channel plumbing; the dollar is not
  a transferable asset.
- **Keep founder-funded reserve indefinitely**: caps supply at one
  balance sheet and leaves the ledger's most-cited scaffold with no
  exit. Rejected by the mission's closing clause.
