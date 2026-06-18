# ADR-0006: SatUSD reconceived as a pre-covenant BTC/USD options market

- **Date**: 2026-06-17
- **Status**: accepted
- **Authority**: `docs/MISSION.md` v3 ("Two eras") via ADR-0001.
  **Refines and extends ADR-0005** — its unilateral-redemption core
  holds, but the core object is reconceived from a "redemption-bearing
  dollar note" into a **fixed-maturity option**, and the product into a
  **market**. Relates to [ADR-0007](ADR-0007-covenant-wall-two-eras.md)
  (the impossibility result that forces the two-era split). Design home:
  `docs/spec/07-redemption-notes.md`, PRD v2.0. Research:
  `docs/research/2026-06-design-journal.md`,
  `docs/research/2026-06-landscape-and-bitvm-research.md`.
- **Author**: Jeffery (decisions) + AI agent (research + drafting)

## Context

ADR-0005 settled that SatUSD is a no-transfer, unilaterally-redeemable
holding whose core object is a redemption-bearing note. A 2026-06 design
exploration — driven by trying to give that note an *anytime, current-price*
redemption — hit a structural dead end (the **free-option** problem: a
holder accumulates pre-signed CETs at different prices and cherry-picks
the most favourable), and an 11-agent adversarial debate killed the
candidate "fresh-anchor / rolling-pre-sign" designs. The exploration
converged on a cleaner conception, which turned out to be economically
isomorphic to Vitalik Buterin's 2026-06 options-based-synthetic proposal
(P/N split, maturity settlement, no liquidation, slow oracle).

## Decision

Pre-covenant SatUSD is a **fixed-maturity, BTC-collateralized European
option**, settled by a DLC at maturity against an oracle BTC/USD
attestation — and the *product* is a **self-custodial, KYC-free options
market**, not a circulating dollar.

1. **The instrument is an option pair.** Locked collateral `Q` splits
   into a **P leg** (protective / dollar-stable — lock value, hedge,
   synthetic short) and an **N leg** (leveraged-long). `P + N ≡ Q`, so
   **there is no liquidation**.
2. **Maturity-only settlement ⇒ free-option = 0.** One oracle event, one
   price, no rolling, no accumulable stale authorizations. The "anytime
   current-price redemption" goal is **dropped** — it is the source of
   the free-option, and is deferred to the covenant era (ADR-0007).
   Redemption stays **unilateral**: the holder broadcasts the pre-signed
   maturity CET alone, or anyone may, or a **holder-only CSV fallback**
   fires. The permanently-offline-holder maturity floor is the Phase-1
   hard gate (PRD FR-4).
3. **Collateral custody is trustless via 2-of-2 MuSig2 `Q`** (Phase 1):
   the LP cannot move or seize `Q` before maturity. (Without it, v0's
   single-key `Q` is LP-trusted — the gap ADR-0005's design still
   carried.)
4. **LPs are market-makers, not directional bulls.** An LP takes the
   other side and manages its own risk however it likes; a delta-neutral
   **CEX hedge is one optional choice with no formal connection to the
   framework**. This widens who can be an LP (professional market-makers,
   not only bitcoin bulls) and answers "who takes the other side?". The
   holder never touches a CEX, and the holder's backing is the
   **per-position locked `Q`**, never any LP hedge (the distinction from
   Ethena-style delta-neutral synthetics).
5. **Liquidity = a single-hop secondary market within a pseudonymous
   membership club + an order book**, trades atomic (Lightning/PTLC), the
   LP recycling `Q`. Open, permissionless, fungible circulation is
   covenant-era (ADR-0007).
6. **Not a perpetual.** We deliberately reject the 10101 path (a synthetic
   dollar built as an inverse perpetual): it requires an
   always-in-the-loop **coordinator** counterparty, **liquidation**,
   funding, and **DLC channels** — the operational complexity that killed
   10101 (force-close ≥ 2 days, routing failures). A fixed-maturity option
   keeps the LP out of the redemption loop, with no liquidation and no
   channel.

## Consequences

- The already-validated two-input burn⟺claim DLC settlement is **reused**
  as the maturity CET — not discarded.
- Backing is **per-position `Q`** (`P + N = Q`); no shared pool, no
  global-solvency trust — verification is per-position (PRD FR-6).
- The product shifts from "a circulating dollar" to "a self-custodial
  BTC/USD options market" — more differentiated, and an **open niche**:
  no live competitor combines self-custody + no-issuer + no-liquidation +
  DLC-maturity settlement (landscape research). The holder's value is
  **hedge / short / lock-in without KYC or wrapped tokens**, not yield
  (yield accrues to the N leg and the LP's spread).
- Honest costs (carried into PRD §6/§9): held-to-maturity (not cash),
  tail risk below over-collateralization, permissioned (pseudonymous)
  club, single-hop transfer only.

## Alternatives considered

- **Fresh-anchor / rolling-pre-sign** (designs B′/B″) for anytime
  current-price redemption — killed; see ADR-0007 and the 11-agent
  debate.
- **A perpetual** (10101) — rejected (decision §6).
- **Wrapping in a fungible L2 / federation token** — sacrifices the
  trustless core; rejected.
