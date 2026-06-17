# SatUSD over Lightning (the BTC speed layer)

- **Spec**: 08
- **Version**: 0.1-draft
- **Authority**: `docs/MISSION.md` v2 via ADR-0001
- **Depends on**: 00 (conventions), 02 (rail standard), 03 (oracle),
  07 (redemption notes)
- **Status**: design — positioning + layered plan from the 2026-06-16
  session; bounded by the LN limits measured on the live signet litd
  testbed (§4). Normative language is aspirational until prototyped.

The key words MUST, MUST NOT, SHOULD, MAY are RFC 2119.

## 0. Positioning

**Lightning is SatUSD's BTC speed layer, not a SatUSD-circulation layer.**
The trustless guarantee — every note's unilateral, oracle-gated,
on-chain DLC redemption (spec 07 §3) — **stays on L1**. Lightning adds
*speed and cheapness to the BTC side*.

This follows directly from spec 07's model: SatUSD is held, not handed
peer-to-peer; **spending = redeem to BTC, pay BTC.** So a SatUSD-
denominated payment *is* a BTC-over-Lightning payment funded by a
redemption: **hold dollars (SatUSD, L1) → spend BTC (Lightning).** LN
never has to carry SatUSD itself for this to work.

A note that LN could *technically* carry SatUSD as a Taproot-Asset channel
balance — but that is the **transferability frontier** SatUSD deliberately
defers (spec 07 §8–§9); it reintroduces the binding/fungibility problem
and belongs to the covenant era (§4). This spec is about LN for the *BTC*
leg, with SatUSD staying an L1 note.

## 1. Layer 1 — LN as the BTC payment rail (clean; buildable now)

The minimal, fully-trustless integration:

1. The holder redeems on-chain (spec 07 §3 `redeem_tx`) → BTC at `K`.
2. That BTC is spent over Lightning (mature, BTC-native).

The redemption is L1-paced (block time + the unilateral guarantee); the
*resulting* BTC moves at LN speed. The `redeem_tx` payout key `K` MAY be a
channel-funding or LN-adjacent output so the redeemed BTC lands ready to
route. **No new trust, no new primitive** — redemption is exactly spec 07,
LN is just where the BTC goes next.

## 2. Layer 2 — fast cooperative redemption (LP advance; a design item)

To make *spending feel instant*, an LP MAY **advance BTC over Lightning**
against a holder's redemption — a submarine-swap-style cross-layer
exchange: LN-in (the LP pays the holder BTC now), on-chain-out (the LP is
made whole when the holder's redemption settles).

**Honest complication.** Because a note is **non-transferable** (spec 07
§8), the LP cannot simply *take* the note or its redemption (the pre-signed
CET pays the holder's `K`, not the LP). So a clean atomic advance is not
free; the realistic shapes are:

- **0-conf advance**: the LP pays over LN once the holder's on-chain
  `redeem_tx` is in the mempool, accepting 0-conf risk (priced into the
  fee); the LP recoups from the confirming redemption (holder directs the
  payout, or repays, gated by the LN HTLC preimage).
- **Cross-layer HTLC**: the on-chain redemption output and the LN payment
  share a hash/point so neither party can cheat (reverse-submarine-swap
  pattern), with a timeout refund.

Either way, **the on-chain unilateral redemption (spec 07 §3) is the
always-available backstop**: if the LP does not pay over LN, the holder
simply redeems on-chain themselves. The holder is never stuck or cheated;
the LN advance only ever *speeds up the happy path*.

The exact atomic construction (and whether the non-transferability makes a
trustless instant advance possible, or it remains a priced 0-conf service)
is **open** (§5).

## 3. Amount & routing strategy

Bounded by the measured LN limits (§4), SatUSD's LN use is **small-and-
fast on LN, large on-chain**:

- **Small / instant** (≲ the carrier cap, single/short hops): the BTC leg
  rides Lightning.
- **Large** (or beyond the asset-channel carrier cap): settle the BTC leg
  on-chain (the L1 `redeem_tx` already produces on-chain BTC).
- Quotes for any LP-advanced redemption (Layer 2) follow the rail
  standard's `Quote` + `price_dev_bound` (spec 02) so the holder verifies
  terms before accepting.

## 4. Measured LN limits (2026-06-16 signet litd testbed)

These bound today's design and MUST be respected by an implementation
(see [[project-ln-findings]]):

- **Asset keysend is direct-peer-only** (selects the channel by
  destination; not multi-hop), ~0.39 s.
- **Invoice (multi-hop) ≈ 3–4 s**; `rfq_id` reuse cuts the payment leg ~5×.
- **Dust floor ≈ $0.23** (354 sat).
- **Carrier cap ≈ $64** (tapd's 100 000-sat `ChanAmt` is hard-coded).

So today LN favours **small, near-direct** BTC movements; the asset-channel
path is constrained, which is another reason SatUSD keeps the *asset* on
L1 and uses LN for the *BTC* leg.

## 5. Out of scope here (covenant-era / frontier)

- **SatUSD circulating as an LN asset-channel balance** — the
  transferability frontier (spec 07 §8–§9); needs the covenant-era model.
- **DLC-in-channel** (the redemption DLC negotiated/updated off-chain
  inside an LN channel, on-chain enforcement as backstop) — research
  frontier; aligns but well beyond v0.

## 6. Open items

1. **Layer 2 atomicity**: can a non-transferable note support a *trustless*
   instant LN redemption advance (cross-layer HTLC), or is it necessarily a
   priced 0-conf LP service? Pin the construction.
2. **Payout-key `K` shape**: on-chain vs channel-funding vs LN-HTLC output;
   how `redeem_tx` best feeds the BTC into a routable position.
3. **Carrier cap**: whether SatUSD needs the BTC leg at all over asset
   channels, or only BTC-native LN (no TA in the channel) — the latter
   sidesteps the 100k-sat carrier limit entirely.
4. **Quote/verification reuse**: apply spec 02 `Quote` semantics to LP
   redemption advances so the holder verifies before accepting.
5. **DLC channels for the Layer-2 advance**: a DLC channel's `settle`/`renew`
   flow is LN-native and is a candidate construction for the cooperative
   advance (§2) — the in-channel DLC updates off-chain, with an on-chain
   force-close as backstop. Two caveats from the 2026-06 research:
   (i) **the channel backstop is WEAKER than v0's single-tx redeem** — it is
   a revocation+penalty model (`buffer → CSV → CET`, safe only on the latest
   state, watchtower required; see spec 07 §10.4 cost (c)), so a channel here
   trades redemption strength for off-chain speed rather than adding to it;
   (ii) **10101 retreated from LN-embedded DLC channels to standalone ones**
   (the embedding proved operationally heavy) — standalone is the likelier
   vehicle. Ref: Crypto Garage "Scaling DLC", `rust-dlc`, Suredbits
   "Transferring In-Channel Lightning DLCs".
