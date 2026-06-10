# ADR-0002: Capacity, not identity — formula-bounded rail exposure replaces asset-class tiers

- **Date**: 2026-06-10
- **Status**: accepted
- **Authority**: `docs/MISSION.md` v2, principles 1–3
- **Supersedes**: the senior/experimental rail-admission model in
  spec 02 v0.1 first draft (§6)
- **Author**: Jeffery (decisions) + AI agent (drafting)

## Context

Spec 02's first draft contained a binary asset-class model: a
"senior" SatUSD carried only by graduated rails, plus
"experimental" classes as separate assets for rail trials. Two
problems surfaced on review:

1. **Graduation needs an adjudicator.** Whether via an on-chain
   allowlist committed in TA group metadata (whoever holds the
   group key becomes the rail gatekeeper — a decree-maker at the
   protocol's center) or via de-facto client defaults (a soft
   monopoly), someone ends up deciding identity. This contradicts
   MISSION principle 2.
2. **Asset-class splits fragment fungibility.** Money's network
   effect requires one fungible unit; per-rail asset classes would
   keep volume from aggregating — directly against the goal of
   growing total volume toward the mission.

The reframe: stop asking *"is this rail good?"* (identity —
requires a judge) and ask *"how much harm can this rail cause?"*
(quantity — answerable by formula).

## Decision

1. **Capacity formula.** Per epoch, each rail's draw on the common
   reserve is capped at
   `capacity = α × (retained_fees_cum + lp_stake)`, with α a
   spec-pinned constant (initially 0.5), never a manifest field.

2. **No-profitable-rug invariant.** α < 1 guarantees the maximum
   extractable value of a one-epoch attack is strictly below the
   attacker's verifiable sunk cost. Track record becomes a costly
   signal (Spence): wash volume costs real retained fees at 1:1.

3. **No time factor in the protocol formula.** The invariant holds
   regardless of rail age; age heuristics belong to the advisory
   curation layer where they don't slow honest capital.

4. **Fee split: retain + service.**
   - `retain_bps` accrues to the **common reserve** (implemented as
     a reimbursement haircut: LP reimburses at
     `burned_amount × (1 − retain_bps)`, the difference stays in
     reserve raising CR for all holders). Economically equivalent
     to a burn, operationally cleaner, and verifiable from
     reimbursement artifacts. Only this component feeds the
     capacity formula — it cannot be faked by self-dealing.
   - `service_bps` pays named per-settlement service providers
     (oracle, DA mirror, broadcaster) at the edge — no treasury,
     no entity. This is the carrier of the **cost principle**
     below.

5. **Cost principle.** Protocol operating costs are ultimately
   paid from protocol fees. Bottom-up estimate (2026 prices):
   steady-state infra ≈ $10k–30k/yr early; with audit amortization
   ≈ $60k–180k/yr in the scaling phase. At a 5 bps service share,
   coverage thresholds are ≈ $55k/day volume (steady state) and
   ≈ $820k/day (incl. audits) — reachable within Phase 1. Until
   then the founder funds the gap (consistent with the self-funded
   reserve decision). Funding mechanism for lumpy costs (audits,
   development) without an entity is **explicitly deferred**;
   candidates recorded: grants, a voluntary dev-fee manifest line
   with client defaults, founder funding.

6. **LP stake is BTC-only.** SatUSD-denominated stake would be
   reflexive (stake value correlated with the failure it insures
   against) and is non-conforming.

7. **Enforcement = epoch allotment with three-stage hardening**
   (spec 04): per-rail reserve tranches sized by the formula each
   epoch (~2016 blocks);
   - Stage 1 (manual, transparent): founder runs the open-source
     allotment script; anyone re-runs it to verify; deviation is
     public challengeable evidence. Scaffolding per MISSION.
   - Stage 2 (pre-signed ceremony): at each epoch boundary, a
     signing ceremony pre-signs the next epoch's tranche tree
     (reimbursement paths gated by optimistic challenge windows,
     Clementine-style; unspent tranches timelock back to reserve).
     Intra-epoch over-cap spends are unsigned and thus impossible;
     trust shrinks to the boundary ceremony.
   - Stage 3 (covenant/BitVM): the formula and burn-proof
     verification move into spend conditions; the ceremony
     disappears.
   The epoch exists because capacity grows over time while any
   enforcement structure is static once erected: the epoch is the
   refresh cadence, identical across all three stages — which is
   what makes the transition seamless.

8. **Experimental assets are demoted, not deleted**: separate TA
   assets are reserved for experiments changing the asset rules
   themselves (reserve model, CR formula, burn semantics), not for
   rail trials. Rail trials are safe on the canonical asset under
   capacity bounds.

9. **Quote transport is not standardized** — only the Quote
   payload. Transport (Nostr, HTTP, etc.) is a rail-implementation
   choice.

## Consequences

- One fungible SatUSD; volume aggregates; no graduation cliffs; no
  gatekeeper anywhere in the rail lifecycle.
- New rails enter with near-zero capacity and grow continuously by
  serving honestly — the emergence mechanism *is* the growth
  curve.
- Curation lists (Token-Lists model) become a competitive advisory
  market; disclosure requirements in spec 02 §6.4 are the only
  normative part.
- Reserve-side reimbursement pricing and the reference marker
  become the binding open design items (spec 04 / spec 03); the
  capacity-amplification refinement
  (`/ price_dev_bound`) waits on them (spec 02 §9.3).
- Stage 1 places formula execution on the founder: enumerated as
  scaffolding with its removal criterion (Stage 2 ceremony
  operational).
