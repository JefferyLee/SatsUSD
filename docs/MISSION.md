# SatUSD: A Bitcoin-Native Bridge to a Bitcoinizing Future

*Version 3 — restructured into two eras (pre-covenant / post-covenant)
after the 2026-06 design exploration. All other documents in this
repository derive from, and are subordinate to, this one. 中文版本：
[MISSION.zh.md](MISSION.zh.md)*

## Two monies, two failures

Bitcoin is the most reliable money ever designed: fixed supply,
no discretionary issuer, permissionless, censorship-resistant,
custody-sovereign. As money, its principal failure today is
**purchasing-power volatility**: its day-to-day price moves too
much for anyone to quote a salary, a contract, or a cup of coffee
in it. It cannot yet serve as a unit of account.

The US dollar has the opposite profile. Its short-term purchasing
power is stable enough to quote prices in. But it is issued by
political authority, inflated at the issuer's discretion, and
increasingly programmable into a surveillance and control
apparatus — account freezes, transaction monitoring, sanctioned
addresses, programmable CBDC restrictions. These are the lived
experience of millions in 2026.

Both monies fail. Bitcoin fails as a present-day unit of account.
The dollar fails as a long-term store of freedom. The question
this project answers is: **can a single instrument inherit the
strengths of both, while shedding the weaknesses of either?**

## Money is an information system

Money's deepest function is neither storage nor payment. It is
**information**: prices denominated in a common unit are the
distributed signals by which billions of strangers coordinate
production, consumption, and exchange. This price-signal function
lives not in any ledger but in the habits of every mind that
quotes a price — which is why it is the hardest function of money
to migrate.

Fiat performs this function today — that is the real reason it
survives. But fiat's signal source is owned: inflated at will,
surveilled at scale, being rebuilt into an instrument of control.
A free market running on captured signals is free in name only.

**The ultimate mission of this project is to migrate the
price-signal function of money from fiat to bitcoin** — not to
migrate balances (possible for fifteen years) but to migrate the
function: the unit people quote in, the signals markets
coordinate by. That is the north star. The rest of this document
is honest about how far today's Bitcoin lets us travel toward it,
and what we build in the meantime.

## Two eras, one direction

The full vision — a self-custodial, permissionless, **fungible,
freely-transferable** dollar token that is **perpetually pegged**
to BTC/USD, redeemable and spendable at any moment, circulating
over Lightning, generating the canonical price signal — **cannot
be built trustlessly on Bitcoin as it exists today.** This is not
an engineering gap to grind through; it is a property of the base
layer — and we did not assume it, we *proved* it. We, like others
in the Bitcoin community before us, spent real effort trying to
build the full, post-covenant vision on the Bitcoin that exists
today; only by exhausting those attempts (the dead-ends are
documented in `docs/research/2026-06-design-journal.md`) did we
establish, rigorously, where the wall stands:

- Bitcoin script **cannot read a live oracle price**, so a
  redemption authorization cannot be computed at spend time; it
  must be **pre-signed** for a fixed outcome in advance.
- Bitcoin has **no way to expire a signature**, and **cannot
  re-bind a claim or constrain a payout amount to an unknown
  future holder**. So a pre-signed claim cannot freely circulate
  to arbitrary recipients, and a holder cannot redeem at "the
  current price" without accumulating an exploitable menu of
  stale prices.

Both gaps are closed by the same future capability: **covenants**
(notably `CHECKSIGFROMSTACK` to verify an oracle signature in
script, plus amount/output introspection such as
`OP_CHECKCONTRACTVERIFY`), which let Bitcoin enforce "redeem at
the latest attested price, pay whoever holds the asset" without
trusting any party. **None of these opcodes is active on Bitcoin
mainnet.** Betting the project's existence on a soft fork that may
take years — or never come — would be a strategic error.

So SatUSD has **two eras**, and this document describes both
honestly:

- **Pre-covenant (now): a pure, permissionless, KYC-free BTC/USD
  options market.** A genuinely useful instrument that today's
  Bitcoin *can* enforce trustlessly — and the foundation the full
  vision is later built on.
- **Post-covenant (the horizon): the bridge.** The fungible,
  freely-transferable, perpetually-pegged, spendable dollar that
  migrates the price signal — achievable when covenants land.

The same direction runs through both: self-custody, no issuer in
the redemption path, no fiat, bitcoin-only reserve, no kill
switch, everything verifiable by anyone. What changes between the
eras is not the values — it is how much of the vision the base
layer permits.

**The pre-covenant era is not a consolation prize.** Having ruled
out everything today's Bitcoin cannot do, what remains is something
it genuinely *can* enforce trustlessly — and that no one else
offers. We build it because we believe it is valuable in its own
right, for the people it serves now, not merely as a placeholder
for the bridge.

**How both eras are built.** Development runs against a single
custom signet with the covenant opcodes activated. The pre-covenant
option market is built and hardened *first*, using only the rules
that already hold on Bitcoin mainnet (the covenant opcodes are
present on the testbed but left unused) — so it is deployable
today. The post-covenant SatUSD is then developed against those
activated opcodes, so that when mainnet covenants land, the bridge
is ready to ship rather than only imagined.

---

## Pre-covenant SatUSD (today): a pure, permissionless BTC/USD options market

Strip away what today's Bitcoin cannot do, and a real product
remains — one that *no one else offers*.

**The underserved holder.** There are many bitcoiners who refuse
centralized exchanges (KYC is a privacy liability), refuse wrapped
tokens like wBTC/tBTC (custodial and bridge risk), and so leave
their bitcoin idle: they cannot hedge, cannot lock in a gain,
cannot take a short or a leveraged-long view, without surrendering
either their privacy or their self-custody. SatUSD gives them a
way to **take a position on their own bitcoin — short, long,
hedge, or lock-in — without KYC, without a wrapped token, without
leaving self-custodied bitcoin.**

**What the instrument is.** A bitcoin-collateralized **option**,
settled by a Discreet Log Contract (DLC) at a **fixed maturity**
against an oracle's BTC/USD attestation. The collateral splits
into two complementary legs that always sum to the locked
bitcoin, so **there is no liquidation**:

- a **protective leg** (the holder who wants dollar stability): at
  maturity it is worth a fixed dollar amount of bitcoin — buying
  it locks in dollar value / hedges / synthetically shorts BTC;
- a **leveraged leg** (the holder who wants upside): it takes the
  bitcoin appreciation above the strike — a self-custodial
  leveraged-long.

The protective-leg holder gets a stable dollar claim; the
leveraged-leg holder gets bitcoin upside; neither can be
liquidated; the oracle is needed only once, at maturity (no
real-time price feed, hence little manipulation surface).

**Redemption is unilateral and unstoppable.** The settlement is
pre-signed at issuance; at maturity the holder broadcasts it
**alone**, on-chain — no issuer signs, no one can freeze or
refuse. This trustless, holder-sovereign exit is the core that
survives into both eras.

**Liquidity providers take the other side; how they manage risk is
their own affair.** Every position is taken by an LP — and how an
LP handles its resulting bitcoin exposure is entirely its own
business, external to the framework. The framework **neither
requires nor has any formal connection to any exchange.** An LP may
hold a directional view, hedge on-chain, leave it unhedged, or — a
natural choice for a professional market-maker — go **delta-neutral**
by hedging on a centralized exchange and earning the **spread**, the
more trading the better. That this is merely *possible* widens who
can profitably be an LP (professional market-makers attracted by the
yield, not only bitcoin bulls), which answers the question that sank
earlier attempts ("who wants the other side?") — and such an LP
*brings CEX liquidity depth to on-chain users* who would otherwise
never touch an exchange. But it is an *option*, not a
dependency: **whatever an LP chooses, the holder never touches an
exchange, and the holder's backing is always the on-chain locked
bitcoin, never any LP hedge** — distinguishing SatUSD from
delta-neutral synthetics (e.g. Ethena), where the holder *is* the
one exposed to the hedge.

**Exiting before maturity.** A holder who does not want to wait can
unwind through the LP: the LP **cosigns an atomic reissue** — it
closes the seller's position and mints a fresh one to the buyer
(under the buyer's own keys), the two legs swapped atomically against
payment. The LP is online and can **refuse but never steal** — the
fresh settlement is oracle-gated to the buyer's key, verifiable
before any payment changes hands. This is market-making and
collateral recycling, not a transfer of the original claim.
**Open, permissionless, peer-to-peer circulation of a live option —
freely transferable to arbitrary recipients — is covenant-era**: we
proved today's Bitcoin cannot extinguish a seller's pre-signed claim
nor re-bind a payout to an unknown future holder, so a *trustless
P2P transfer is not something we ship now*.

**An open-source framework, not a company.** A single market is
necessarily bounded. So the deliverable is an **open framework
anyone can deploy to run their own market**. Many small markets —
each anchored by an institution or a bitcoin-rich whale acting as
the market-making LP, monetizing idle bitcoin without lending it
out or selling it — federate into meaningful aggregate volume.
**This project does not seek to commercialize a product or extract
rent; it seeks to give the Bitcoin community a useful, secure
technical framework.** Success is measured by usefulness and
correctness, not by TVL.

**Honest scope and limits (pre-covenant).**

- It is an option you **hold to maturity** (or unwind early through
  an LP-cosigned reissue), not a circulating cash-like token.
  Liquidity before maturity comes from that LP path and a maturity
  *ladder*, not from spending or freely transferring the instrument
  itself.
- It is **not fungible across series** — different strikes and
  maturities are different instruments; fungibility is achieved
  only within a standardized series, and full fungibility is
  covenant-era.
- It is **not freely transferable**: open, permissionless P2P
  circulation of a live option to arbitrary holders is covenant-era;
  pre-covenant, exit before maturity goes through an LP.
- Backing is **over-collateralization**, not a guarantee against a
  catastrophic crash: below the collateral floor the holder bears
  the tail, as in any honest bitcoin-collateralized synthetic
  (DAI/LUSD). This is stated plainly, not papered over.

**The trust model, in three honest layers.** SatUSD's trust does not
collapse to a single number; it decomposes, and only one layer is
oracle-bounded.

- **Unbounded / trustless (oracle-independent).** Self-custody (the
  bitcoin never leaves mainnet), unilateral maturity settlement (no
  counterparty can freeze, seize, or refuse), and the offline floor
  (collateral recoverable with no oracle at all). This is the genuine
  differentiator no fiat stablecoin has — USDC/USDT always carry a
  freeze node; SatUSD has none.
- **The oracle ceiling (settlement-value correctness).** One fact —
  the BTC/USD price at maturity — must be attested, and that is
  **bounded, *disclosed* trust**, never a "trustless oracle." We
  engineer the bound as tight as the rest of the industry and
  tighter: **k-of-n independent reputable signers**, each
  **cross-checked against public reputable feeds**, behind an
  **optimistic bonded dispute window** that lets anyone challenge a
  lie, with **equivocation-slashing** as the crypto-economic floor.
  This is the *same reputation-bounded ceiling Chainlink and Pyth
  secure billions under* — high in practice, rising over time as
  signers multiply and SatUSD's own volume eventually blends in.
- **Three honest residuals (disclosed, not eliminated).** A
  very-well-resourced *external-short* attacker is the untunable tail
  (bounded by signer independence + a notional cap + monitoring, not
  zeroed); assembling a credible reputable committee is a real
  operational lift, not a design footnote; and the safe notional is
  **trust-capped, and grows with the trust**.

The project's possibility is therefore the product of two factors:
the **industry-common oracle ceiling** that every crypto-dollar
shares, multiplied by the **orthogonal, unbounded custody/censorship
edge that no fiat stablecoin has**. The oracle is not our weakness
relative to competitors — it is the shared constraint of every
crypto-dollar; SatUSD simply removes the extra freeze node they keep.

---

## Post-covenant SatUSD (the horizon): the bridge

When covenants activate, the constraints above dissolve, and
SatUSD becomes what the mission always aimed at: a **fungible,
freely-transferable, permissionless** Taproot Asset, **perpetually
pegged** to BTC/USD (redeemable at the live price, no fixed
maturity), **spendable** — payable peer-to-peer and over
Lightning — and redeemable unilaterally by whoever holds it, with
no issuer, no club, no KYC.

At that point it is **a bridge** — not just a stablecoin. The
dollar's fortress is not the central bank; it is the habit of
billions of minds quoting prices in dollars. SatUSD does not
assault that network effect; it hollows it out. The user keeps the
dollar habit — familiar denominations, stable quotes — while the
substance beneath becomes bitcoin: reserve in bitcoin, never
leaving Bitcoin L1; settlement in bitcoin; custody in the holder's
own keys. **Substance migrates first; the habit migrates last —
across a bridge that already exists.** The dollar peg is the path,
not the destination.

The transition the bridge enables — recognized in hindsight by
metrics, never declared:

- **Phase 0 — We exist.** Small volume, prices pinned to external
  sources.
- **Phase 1 — Real volume.** The internal market begins generating
  its own data; external oracles remain the reference.
- **Phase 2 — The internal market becomes canonical.** SatUSD's
  own trade history is the most authoritative BTC/USD price on
  Bitcoin L1 — the bitcoin economy, for the first time, generating
  its own price signal. External sources demote to sanity checks.
- **Phase 3 — Denomination begins to flip.** Commerce is
  increasingly quoted in SatUSD and settled in bitcoin; measured
  volatility shrinks as the speculative share of flow shrinks.
- **Phase 4 — The bridge retires.** When bitcoin is a sufficient
  unit of account, SatUSD's work is done; issuance wanes; the
  instrument winds down by attrition. **A bridge succeeds when
  traffic no longer needs it.** We state this so that no one —
  including us — can later pretend this project was meant to live
  forever and collect rent.

**Self-referencing is the definition of success, and it is not
Terra.** A stablecoin that forever depends on an external oracle
has not escaped the system it claims to escape. Deriving the
canonical price from SatUSD's own on-chain activity is therefore
not an optimization — it is the mission achieved. And the
circularity that destroyed Terra's UST was in its *collateral*
(UST backed by LUNA, whose value derived from UST). **SatUSD's
collateral is bitcoin** — value exogenous to SatUSD's existence;
redemption transfers bitcoin, mints nothing, dilutes nothing. What
is self-referenced is **information** (the price signal), and only
after the internal market earns authority through years of
cross-checked operation. A system whose information is
self-generated but whose value is exogenous has the failure modes
of any collateralized system — undercollateralization in a crash,
thin-market manipulation while young — known, bounded, and
engineered against, not Terra's reflexive collapse.

**We put no dates on these phases.** They are recognized by
metrics, not calendars — and they are gated, first of all, on the
covenant capability that opens this era at all.

---

## What SatUSD is not

**No centralized issuer.** No company that can freeze an address.
You trust a protocol, not a firm. (Pre-covenant, an LP can decline
to *open* a position or to cosign an *early-exit reissue* for you —
but it can never seize, freeze, or refuse to honor an option already
in your hands; your unilateral maturity redemption is yours alone.)

**No fiat reserve.** No dollars, no Treasury bills, no fiat
instrument of any kind. The reserve is bitcoin only.

**No yield on the holding itself.** Paying yield would require
deploying the reserve into yield-bearing positions, compromising
the redeem guarantee or re-importing fiat dependencies. The
protective-leg holder gets stability, not interest; the return for
taking risk accrues to the leveraged leg and to the LP's spread.

**No permission layer on the holder's bitcoin.** No KYC, no AML
gate, no freeze function, no admin key over the holder's
self-custodied bitcoin or their unilateral redemption. (The
pre-covenant market is **free-entry and permissionless** — no club,
no registration; an LP that *chooses* to hedge on an exchange does
its own KYC there as a private matter unconnected to the protocol —
neither touches the holder's custody or their right to redeem.)

**Not optimized for institutions.** We optimize for the
individual who treats monetary sovereignty as an end in itself —
the bitcoiner who will not give up self-custody or privacy to do
something with their bitcoin.

## Why existing products fall short

Every dollar-instrument on the market makes a compromise SatUSD
refuses — and the bitcoin-native ones each miss the specific niche
SatUSD fills:

- **Centralized + fiat-backed** — USDT, USDC; on Bitcoin, **USDB**
  and **Citrea ctUSD** (licensed issuer, Treasury reserves). A
  kill switch wearing a brand; the trust never left fiat.
- **CDP with liquidation** — on Bitcoin, **Ducat, bitSmiley,
  Satoshi, Avalon, BOB**. Over-collateralized debt that force-sells
  in a crash — exactly the liquidation SatUSD's option structure
  removes.
- **Delta-neutral synthetics with exchange counterparties** —
  **Ethena (USDe)**, **Stablesats**, **Hermetica**. The holder's
  backing *is* the exchange hedge; counterparty and funding risk
  are the holder's. SatUSD's holder is backed by on-chain bitcoin;
  any LP hedge is the LP's own private business, optional and
  unconnected to the protocol.
- **The wrong chain** — value that settles off Bitcoin L1 inherits
  someone else's security.
- **Algorithmic** — Terra's UST: stability conjured from a
  system's own token, a proven failure mode.

The closest thing to pre-covenant SatUSD — a self-custodial,
issuer-less, no-liquidation, DLC-settled bitcoin synthetic — was
**10101**, which shut down in 2024: it built it as a *perpetual*
(requiring an always-in-the-loop coordinator, liquidation, and
DLC-channel plumbing that proved operationally fatal) and died of
that complexity plus lack of traction. SatUSD takes the deliberate
opposite path: a **fixed-maturity option** (no coordinator in the
redemption loop, no liquidation, no channel), shipped as an
open-source community framework rather than a venture-funded
company that must grow or die.

This exact combination — self-custodial, no issuer in the
redemption path, no liquidation, bitcoin-only reserve,
maturity-settled, open-source — exists nowhere else today.

## How, in principle

The mission constrains the mechanism. Four principles, in both
eras:

1. **Everything verifiable by anyone.** Every claim — reserve,
   supply, lineage, price, an LP's solvency — must be checkable by
   client software against Bitcoin's chain, not asserted by any
   authority.
2. **Trust is priced by a market, not chosen by a decree.**
   Competing LPs, oracle rails, and markets; users pick; market
   share is the judgment.
3. **Liveness is bought, not assumed.** Wherever the design needs
   someone to act, it must suffice that *anyone* may act, paid by
   the protocol's economics — never that a specific party must.
4. **Everything temporary exits.** The founder exits — the
   framework runs without its creators. Transitional controls
   exit — scaffolding is enumerated and demolished. The bridge
   itself exits — Phase 4. Nothing here is permanent except the
   bitcoin underneath.

## A standing invitation

This is a bet on a future that is already arriving: bitcoin's hash
rate compounds, its supply is set in stone, the dollar's
purchasing power erodes and the erosion accelerates, CBDCs are
prototyped in two dozen jurisdictions, political account freezes
are no longer unthinkable in liberal democracies. The world is
sorting into people who can opt out of monetary politics and
people who cannot. **SatUSD exists to make the first category
larger** — and to do it as an open gift to the Bitcoin community,
not a business.

We do not seek venture capital, regulatory approval, or
institutional endorsement. We seek:

- **Bitcoin developers** who see that the next decade is about the
  financial infrastructure people live inside, not price-go-up.
- **Cryptographers** willing to engage Bitcoin-L1 oracle design,
  threshold signing, adaptor-signature constructions, and the
  covenant primitives that open the second era.
- **Users** who have felt the cost of fiat coercion and will use
  early, imperfect tools so the next generation has better ones.
- **Critics** who will tell us bluntly where the design is wrong.
  Well-articulated hostile criticism is worth more than friendly
  endorsement.

This is an attempt. It may fail. If it does, the failure will be
documented in the open and the next attempt will start further
down the road we cleared. What we build will be open, what we
learn will be shared, and what we believe is written here.

---

*This document defines the project's intent and is the highest
authority in this repository. Technical documents define
implementation. Any implementation choice that contradicts this
document must be revised, or explicitly justified as a deliberate,
temporary compromise on the way to it — enumerated, with its
removal criteria stated. The vision here is intended to outlive any
individual contributor, including its original drafters.*
