# ADR-0007: The covenant wall — fungibility, transfer, and the perpetual peg are covenant-era; the two-era split

- **Date**: 2026-06-17
- **Status**: accepted
- **Authority**: `docs/MISSION.md` v3 ("Two eras") via ADR-0001. Pairs
  with [ADR-0006](ADR-0006-pre-covenant-options-market.md) (what the
  pre-covenant product *is*); this ADR records *why* it is bounded that
  way. Research: `docs/research/2026-06-design-journal.md`,
  `docs/research/2026-06-landscape-and-bitvm-research.md`, and the
  11-agent falsification debate.
- **Author**: Jeffery (decisions) + AI agent (research + drafting)

## Context

We — and many in the Bitcoin community before us — tried to build the
full SatUSD vision (fungible, freely-transferable, perpetually-pegged,
spendable) trustlessly on the Bitcoin that exists today. We did **not**
assume it impossible; we **proved** it, by exhausting the alternatives
(documented in the design journal and an 11-agent adversarial debate that
killed the candidate designs). This ADR records the result and the
project structure it forces, so the dead-ends are not re-explored.

## Decision

1. **The impossibility theorem.** On Bitcoin mainnet as it exists today,
   these five properties **cannot all hold simultaneously** for a
   self-custodial, BTC-collateralized dollar: **(i)** before maturity,
   **(ii)** at any moment, **(iii)** at the current price, **(iv)**
   unilaterally, **(v)** with free-option = 0. At most four hold; one must
   be dropped. *Root cause:* Bitcoin script cannot read a live oracle
   price (so authorizations must be **pre-signed** for fixed outcomes),
   and Bitcoin has **no signature expiry** and **cannot re-bind a claim or
   constrain a payout to an unknown future holder** (so pre-signed claims
   accumulate into a cherry-pickable menu, and cannot freely circulate).

2. **Transfer ⟂ no-one-in-the-loop.** Open, trustless transfer requires
   re-binding a redemption authorization to an *a-priori-unknown*
   recipient; on today's Bitcoin that needs **either an LP to re-sign**
   (a party in the loop, who can refuse) **or a covenant**. Every transfer
   mechanism explored — LP-cooperative re-bind, two-layer fungible TA,
   repricing pairs, adaptor-signatures + per-member nonces (the
   membership-club construction) — bottoms out at this wall. The only
   trustless on-chain punishment primitive (**equivocation = key-leak**)
   can bound or penalize misbehaviour but cannot reach open transfer.

3. **What is deferred to the covenant era.** Fungibility, free /
   permissionless transfer, the perpetual peg (anytime current-price
   redemption), spendable circulation, and the price-signal-migration
   bridge are **covenant-era**. They are unlocked by **covenants** —
   notably `CHECKSIGFROMSTACK` (verify the oracle in script) plus
   amount/output introspection such as `OP_CHECKCONTRACTVERIFY` — **none
   of which is active on Bitcoin mainnet.** Pre-covenant SatUSD therefore
   drops (ii)+(iii)+open-transfer in favour of maturity settlement and a
   bounded single-hop club market (ADR-0006).

4. **Do not bet the project's existence on a soft fork.** Covenant
   activation may take years, or never come. Gating the project on it is
   rejected — hence the **two eras** (MISSION v3): ship the pre-covenant
   product now; treat the bridge as the horizon.

5. **Two-era development on a covenant-signet.** Development runs on a
   single custom signet with the covenant opcodes **activated**. The
   pre-covenant market is built and hardened **first**, using only the
   rules that hold on mainnet today (the opcodes present but unused), so
   it is deployable now. The post-covenant SatUSD is then developed
   against the activated opcodes, so the bridge is ready to ship — not
   only imagined — when mainnet covenants land.

## Consequences

- The two-era split (MISSION v3; PRD v2.0 scoped pre-covenant) is the
  direct consequence of this result.
- **BitVM2/3 and adjacent no-soft-fork covenant-emulation were evaluated
  and rejected for the redemption core** (landscape research): they
  reintroduce a signer committee / capital-fronting operator / multi-day
  challenge windows that violate no-issuer + unilateral; at most their
  *minimal primitives* (bit-commitment; single-step fraud proof) are of
  interest. This is consistent with the project's earlier, now-archived
  BitVM architecture (archive ADR-0018, ADR-007).
- **Do not re-explore as pre-covenant main paths:** fresh-anchor /
  rolling-pre-sign; a 10101-style perpetual; open transfer without a
  covenant; or a platform move to Ethereum (which removes the constraints
  but discards the Bitcoin-native moat into a crowded field — f(x) et al.).

## Alternatives considered

- **Build the full vision now anyway** (fresh-anchor designs B′/B″) —
  proven impossible; killed by the debate.
- **Move to Ethereum**, where the EVM removes the base-layer constraints
  — rejected: the differentiation was always the hard chain; the target
  niche (self-custodial, anti-KYC, anti-wrapping bitcoiners) does not live
  on Ethereum.
- **Wait for covenants before shipping anything** — rejected: do not bet
  existence on a soft fork; ship the pre-covenant product now.
