# SatUSD: A Bitcoin-Native Bridge to a Bitcoinizing Future

*Version 2 — the founding document, rewritten from the seed vision.
All other documents in this repository derive from, and are
subordinate to, this one. 中文版本：[MISSION.zh.md](MISSION.zh.md)*

## Two monies, two failures

Bitcoin is the most reliable money ever designed: fixed supply,
no discretionary issuer, permissionless, censorship-resistant,
custody-sovereign. As money, its principal failure today is
**purchasing-power volatility**: its day-to-day price moves too
much for anyone to quote a salary, a contract, or a cup of
coffee in it. It cannot yet serve as a unit of account — and so
it cannot yet do the one thing a free market needs most from
its money.

The US dollar — and the fiat system it represents — has the
opposite profile. Its short-term purchasing power is stable
enough to quote prices in. But it is issued by political
authority, inflated at the issuer's discretion, and increasingly
programmable into a surveillance and control apparatus. Account
freezes, transaction monitoring, sanctioned addresses,
programmable CBDC restrictions — these are not hypothetical
futures. They are the lived experience of millions of people
in 2026.

Both monies fail. Bitcoin fails as a present-day unit of
account. The dollar fails as a long-term store of freedom. The
question this project answers is: **can a single instrument
inherit the strengths of both, while shedding the weaknesses of
either?**

## Money is an information system

Money's deepest function is neither storage nor payment. It is
**information**: prices denominated in a common unit are the
distributed signals by which billions of strangers coordinate
production, consumption, and exchange — without any of them
needing to know the whole. This is the price-signal function,
and it is the hardest function of money to migrate, because it
lives not in any ledger but in the habits of every mind that
quotes a price.

Be honest about the present: fiat performs this function today.
That is the real reason it survives. Bitcoin does not — nothing
is quoted in it, so it generates no signals. But fiat's signal
source is owned. It is inflated at will, surveilled at scale,
and — with programmable central-bank money — being rebuilt into
an instrument of control. A free market running on captured
signals is free in name only.

**The mission of this project, stated precisely, is to migrate
the price-signal function of money from fiat to bitcoin.** Not
to migrate balances — that has been possible for fifteen years —
but to migrate the function: the unit people quote in, the
signals markets coordinate by.

## What SatUSD is

SatUSD is a bitcoin-collateralized, dollar-denominated
instrument issued natively on Bitcoin L1 via Taproot Assets. A
holder of N SatUSD holds a claim on $N worth of bitcoin in the
reserve — a claim the protocol is built to enforce without
trusted intermediaries, and one that is as strong as the
reserve's over-collateralization, which the mechanism is
designed to maintain.

It is **a bridge** — not just a stablecoin. The dollar's real
fortress is not the central bank; it is the habit of billions of
minds quoting prices in dollars. That network effect cannot be
taken by frontal assault. SatUSD does not assault it. It hollows
it out: the user keeps the dollar habit — familiar denominations,
stable quotes — while the substance beneath becomes bitcoin.
Reserve: bitcoin, never leaving Bitcoin L1. Settlement: bitcoin.
Custody: the holder's own keys. **Substance migrates first. The
habit migrates last — and by then it is migrating across a
bridge that already exists.**

This is intentional scaffolding. **The dollar peg is the path,
not the destination.**

## What SatUSD is not

**SatUSD does not pay interest on holdings.** Paying yield on a
stablecoin requires deploying the reserve into yield-bearing
positions, which either compromises the redeem-anytime guarantee
or routes the yield through fiat instruments that re-import the
dependencies we are escaping. We refuse this trade.

This does not mean participation is unprofitable:

- **Liquidity providers earn fees.** Providing bitcoin to the
  BTC/SatUSD redemption rails earns a share of every redemption
  spread — a real return, paid in real bitcoin, at every
  settlement.

- **Bitcoin appreciates as the economy bitcoinizes.** The deepest
  return comes not from any yield instrument but from holding
  bitcoin through the transition itself. If SatUSD succeeds,
  bitcoin's purchasing power compounds over the horizon of this
  project. **The reward for being early to a bitcoinizing world
  is bitcoin itself.**

**SatUSD holds no fiat reserve.** No dollars, no Treasury bills,
no fiat instrument of any kind. The reserve is bitcoin only.

**SatUSD has no permission layer.** No KYC, no AML gate, no
freeze function, no admin key on the issuance and redemption
paths. The asset is as permissionless as the bitcoin behind it.
Where today's implementation still contains transitional
controls, they are scaffolding — enumerated, justified, and
scheduled for demolition in the technical documents.

**SatUSD is not optimized for institutions.** Institutional
adoption demands regulatory wrappers and audited custodians that
contradict the asset's core properties. We optimize for the
individual: the person denied banking service, the person under
capital controls, the person who treats monetary sovereignty as
an end in itself.

## Why existing stablecoins fall short

Every stablecoin on the market makes at least one of four
compromises that SatUSD refuses:

**1. A centralized issuer** — USDT (Tether), USDC (Circle),
ctUSD (M0/MoonPay on Citrea). A company that can freeze any
address is a kill switch wearing a brand. You trust a firm, not
a protocol.

**2. A fiat reserve** — USDT, USDC, ctUSD; DAI/USDS (Sky,
formerly MakerDAO) indirectly through its USDC and Treasury
backing; even FRAX, the flagship of algorithmic design, pivoted
to full Treasury collateralization in 2025. The trust never
left the fiat system — it was laundered through a token.

**3. The wrong chain** — LUSD/BOLD (Liquity) live on Ethereum;
BTD (Alpen) lives on a rollup. Value that does not settle on
Bitcoin L1 inherits someone else's security assumptions.

**4. Algorithmic stability** — Terra's UST erased roughly $40B
in May 2022. Stability conjured from a system's own token is a
proven failure mode, not a design choice.

**SatUSD makes none of these compromises.** Bitcoin-only
reserve. Bitcoin L1 settlement. Permissionless issuance and
redemption. No kill switch by design — not for us, not for any
government, not for any committee. This combination exists
nowhere else.

## The transition

First, the thesis that makes the path coherent: **volatility is
a property of denomination, not of bitcoin.** Today bitcoin's
price is set in dollar-denominated, speculation-dominated
markets — so "bitcoin is volatile." In a world where goods and
labor are quoted in sats, the measuring stick has switched
sides, and what fluctuates is the dollar. The phases below are
the path between those two worlds. Their boundaries are
recognized in hindsight by metrics — volume, internal-external
price coherence, oracle market share — not declared by anyone.

**Phase 0 — We exist.** Small volume, prices pinned to external
sources. Most users still think in dollars.

**Phase 1 — Real volume.** The internal market begins generating
its own data. External oracles remain the reference; internal
trades begin to cross-check them.

**Phase 2 — The internal market becomes canonical.** SatUSD's
own trade history is the most authoritative BTC/USD price on
Bitcoin L1. For the first time, the bitcoin economy generates
its own price signal. External sources demote to sanity checks.

**Phase 3 — Denomination begins to flip.** Commerce settles in
SatUSD channels backed by bitcoin reserves; transactional demand
for bitcoin grows continuous and two-sided; measured volatility
shrinks because the speculative share of flow shrinks. Prices
start appearing in sats alongside dollars.

**Phase 4 — The bridge retires.** When bitcoin is a sufficient
unit of account, SatUSD's work is done. Holders redeem into the
sat-denominated world; issuance wanes; the instrument winds down
by attrition, the same way it grew — no decree required. **A
bridge succeeds when traffic no longer needs it.** We state this
in the founding document so that no one — including us — can
later pretend this project was meant to live forever and collect
rent.

**We put no dates on these phases.** Forecasts of the future are
wrong by nature, and a schedule serves no one: too cautious, it
deflates; too bold, it embarrasses. The phases above are
recognized by metrics, not calendars.

What we will say is this: the slope is steepening. AI now
compounds the speed of engineering — this project is itself
built that way. More deeply, AI agents are becoming economic
actors in their own right: they hold no dollar habit, cannot
open bank accounts, cannot pass KYC, and choose their money on
engineering merit alone. The network-effect moat this bridge
exists to cross is a phenomenon of human habit — and the
fastest-growing population of new economic actors has no habits
at all. Machine-verifiable, permissionless, programmable money
is not merely available to them; it is the only money they can
natively use.

Adoption runs on the world's clock, not ours — and the world's
clock is accelerating. We assume we will be wrong about
specifics. We are committed to being right about direction.

## Self-referencing: why it is necessary — and why this is not Terra

A stablecoin that permanently depends on an external price
oracle has not escaped the system it claims to escape. If
SatUSD's redemption rate forever depends on what a Coinbase or
a Binance reports, the legacy financial system retains a veto
over SatUSD's operation — a single point of political,
regulatory, and technical attack. Worse: a bitcoin economy that
still needs fiat-side institutions to know what things are worth
has not actually migrated the price-signal function. It has
outsourced it.

Self-referencing — deriving the canonical price from SatUSD's
own on-chain economic activity, secured by Bitcoin's consensus —
is therefore not a technical optimization. **It is the
definition of success.** When the internal market becomes the
authoritative price source, the bitcoin economy is, for the
first time, generating its own signals. That is the mission,
achieved.

Terra's UST is the obvious objection, so let us be precise
about the difference. **UST's circularity was in its
collateral**: UST was backed by LUNA, and LUNA's value derived
from expected demand for UST. Redemption minted LUNA, diluting
the very backing it redeemed against — a reflexive loop with no
exogenous floor. **SatUSD's collateral is bitcoin** — an asset
whose value owes nothing to SatUSD's existence. Redemption
transfers bitcoin; it mints nothing and dilutes nothing. What is
self-referenced here is not value but **information** — the
price signal — and even that only after the internal market has
earned authority through years of cross-checked operation, with
external anchors demoted to sanity checks rather than
dependencies. A system whose information is self-generated but
whose value is exogenous does not have Terra's failure mode. It
has the failure modes of any collateralized system —
undercollateralization in a crash, thin-market manipulation
while young — which are known, bounded, and engineered against
in the technical documents.

Every architectural choice in this project is to be evaluated by
one criterion: **does it move us closer to, or further from, the
state where the external dependency can be removed?**

## How, in principle

The mission constrains the mechanism. Four principles:

1. **Everything verifiable by anyone.** Every claim the protocol
   makes — reserve, supply, lineage, price — must be checkable
   by client software against Bitcoin's chain, not asserted by
   any authority.

2. **Trust is priced by a market, not chosen by a decree.**
   Redemption runs over an open standard of competing rails —
   different oracle designs, speeds, sizes, fees, trust
   profiles. Users pick; market share is the judgment. The
   self-referencing rail does not get switched on by governance;
   it wins when it offers the best terms.

3. **Liveness is bought, not assumed.** Wherever the design
   needs someone to act, it must suffice that *anyone* may act,
   paid by the protocol's own economics — never that a specific
   party must.

4. **Three exits, one philosophy.** The founder exits — the
   protocol runs without its creators. The transitional controls
   exit — scaffolding is enumerated and demolished. The asset
   itself exits — Phase 4 is written above. Nothing in this
   project is meant to be permanent except the bitcoin
   underneath it.

## A standing invitation

This is not a wager. This is a bet on a future that is already
arriving.

Bitcoin's hash rate compounds. Its supply schedule is set in
stone for the next century. The dollar has lost half its
purchasing power since the turn of the century, and the loss is
accelerating. Central bank digital currencies are being
prototyped in two dozen jurisdictions. Account freezes for
political reasons are no longer unthinkable in liberal
democracies. The world is sorting itself into people who can opt
out of monetary politics and people who cannot.

SatUSD exists to make the first category larger.

We do not seek venture capital. We do not seek regulatory
approval. We do not seek institutional endorsement. We seek the
attention of:

- **Bitcoin developers** who recognize that the next decade of
  this technology is not about price-go-up but about building
  the financial infrastructure that lets people live inside it.

- **Cryptographers** willing to engage the unsolved problems of
  Bitcoin-L1 oracle design, threshold signing, and
  trust-minimized settlement.

- **Users** who have personally felt the cost of fiat coercion
  and will use early, imperfect alternatives so the next
  generation has better ones.

- **Critics** who will tell us bluntly where the design is
  wrong. Hostile criticism well-articulated is worth more to
  this project than friendly endorsement.

This project is an attempt. The attempt may fail. If it does, we
will have documented the failure in the open, and the next
attempt will start further down the road we cleared. If it
succeeds, the world will be measurably freer within our
lifetimes — not through revolution, not through politics, but
through the quiet substitution of better money for worse.

Either way: what we build will be open, what we learn will be
shared, and what we believe is written down here.

---

*This document defines the project's intent and is the highest
authority in this repository. Technical documents define
implementation. Any implementation choice that contradicts this
document must either be revised, or be explicitly justified as a
deliberate, temporary compromise on the way to it — enumerated,
with its removal criteria stated.*

*The vision articulated here is intended to outlive any
individual contributor, including its original drafters.*
