# SatUSD Redemption-Bearing Notes (the unilateral-redemption design)

- **Spec**: 07
- **Version**: 0.1-draft
- **Authority**: `docs/MISSION.md` v2 via ADR-0001
- **Depends on**: 00 (conventions), 01 (asset/burn), 02 (rail standard),
  03 (oracle), 06 (vault funding output + CETs)
- **Status**: design — captures the redemption-binding design reached in
  the 2026-06-16 design session; normative language is aspirational until
  pinned by test vectors + a signet prototype.

The key words MUST, MUST NOT, SHOULD, MAY are RFC 2119.

## 0. What SatUSD is — and is not

SatUSD is a **BTC-backed, self-custodied, unilaterally-redeemable dollar
holding** — *not* a circulating payment medium. Its defining property:

> **Every SatUSD note carries a pre-signed, oracle-gated, unilaterally-
> broadcastable DLC redemption — the holder converts it to BTC at any
> time, with no counterparty's permission and no party able to refuse or
> freeze it.** The peg is enforced by the holder's own cryptographic
> redemption right, not by trusting an issuer to honour redemption.

This is the distinction from every other stablecoin: USDC's issuer can
freeze; DAI depends on a live protocol; SatUSD's redemption is a
pre-signed DLC the holder broadcasts alone (spec 02 L1, "the fact itself
is the executor").

**The deliberate trade (§8): to get this purity on today's Bitcoin — no
covenants — SatUSD forgoes free transferability.** A note is *held* or
*redeemed*, never handed peer-to-peer. "Spending" SatUSD = redeeming it
to BTC and paying BTC. **BTC is the medium of exchange; SatUSD is the
trustless stable parking spot with a guaranteed BTC exit.** This aligns
with the Bitcoin-standard view: SatUSD does not compete with BTC as
money; it is a way to hold a stable dollar value without leaving the
trustless Bitcoin world. Transferability + pooled collateral return in
the covenant era (§9) — the *same* capability unlocks both.

## 1. Roles & lifecycle

| Role | Definition |
|---|---|
| **Holder** | Holds a SatUSD note + its unilateral redemption right |
| **LP / issuer** | Sells SatUSD; locks the over-collateralised BTC; pre-signs the redemption CETs; the BTC-price-risk bearer |
| **Oracle** | Attests BTC/USD per spec 03 (one aggregate key, §6) |
| **Broadcaster** | Anyone may complete a redemption / maturity CET (permissionless) |

```
issue   (buy from LP; the note IS a DLC funding output holding the $X SatUSD
         + over-collateralised BTC; LP pre-signs its redemption CETs)
  → hold (a stable dollar position + a unilateral redemption right)
  → redeem ANYTIME  (one tx: spend the note ⟹ its SatUSD burns ⟺ its BTC pays you; §3)
  → maturity = the LP's committed term  (fair-value auto-settle to BTC; §5)
  → renew  (roll to a fresh-term LP before maturity)
```

## 2. Issuance (where SatUSD comes from)

SatUSD is **minted on demand by an LP at the point of sale** — there is no
genesis pre-supply; supply expands when bought and contracts when
redeemed, always backed by locked BTC. The buyer pays BTC; the LP issues
`$X` SatUSD **into a single DLC funding output that holds both the `$X`
SatUSD (a Taproot-Asset commitment) and the over-collateralised BTC** —
the TA-in-DLC-funding-output construction of `docs/proposals/0001`
(a SatUSD technical proposal to dlcspecs / Delving Bitcoin, implemented +
devnet-validated). **That output _is_ the note**: the buyer holds it; the
LP pre-signs its redemption CETs at issuance. (The LP MAY source the
collateral by opening its own vault, spec 06 §2.)

**Risk attribution.** A BTC↔SatUSD swap does not create risk; it
*transfers BTC exposure*. The buyer goes flat-BTC / long-USD; the LP holds
the reserve and is **long BTC + earns fees** — a position a Bitcoiner
holds willingly. No party is wronged; each chose its exposure (盈亏自负).
The LP's only obligation is to fund Q correctly + pre-sign the CETs, both
verified by the buyer at purchase (§3.4).

## 3. The redemption note — the binding construction (core)

The hard problem: bind a pre-signed redemption right to the note such that
(1) redemption is unilateral, (2) burn ⟺ claim is atomic, (3) no
double-redeem. Secret-revelation schemes ("burning reveals `t`") fail —
anything the holder can compute, they can compute *without* burning. The
binding is instead enforced by **a single Bitcoin transaction with the
note as a required input.**

### 3.1 The redemption transaction

```
redeem_tx:
  input:   [ the note — ONE DLC funding output holding the $X SatUSD
             (a TA commitment) AND the over-collateralised BTC ]
  outputs: [ SatUSD → burn sink   (spec 01 §4 — the note's SatUSD is destroyed)
             (X / P) BTC → K       (the holder's payout key)
             change BTC → LP ]
```

- **Atomicity is structural, not cryptographic**: the note (a single
  funding output) is the *one required input*; spending it — an
  oracle-adaptor-locked key-path spend — in the *same* tx both sends its
  SatUSD to the burn sink and pays the holder BTC from its own collateral.
  You cannot claim without burning; you cannot double-claim (the output is
  spent once).
- **Unilateral by construction — no separate LP input**: the collateral
  lives *inside the note*, so redemption needs no LP signature and no LP
  presence at redeem-time (this is precisely why the earlier two-input form
  was discarded — §3.5).
- The redemption amount `X / P` is the dollar face at the oracle price `P`;
  the note is over-collateralised for a worst-case `P` (§7). Below that, the
  payout is capped at the note's balance (a shortfall — partial redemption).

### 3.2 Authorisation

- **The note's key-path spend**: the LP **pre-signs at issuance** a set of
  CETs spending the note's funding output, one per (oracle event, price
  bucket), as **adaptor signatures** locked to that bucket's oracle
  anticipation point (proposal 0001 §3.2; spec 06 `crash_adaptor_point`;
  spec 03 §3). Each commits the whole spend (a `SIGHASH_DEFAULT` key-path
  spend of the note): SatUSD → burn sink, `X/P` → `K`, change → LP.
- **Completion**: the **public oracle attestation** decrypts the matching
  bucket and completes the adaptor; the holder — or any broadcaster, since
  the payout is fixed to the holder's `K` — broadcasts. **No LP input, no
  LP participation at redemption, no adjudicator, no covenant.**

### 3.3 Why this is sound (and why no bond/challenge is needed)

Because the LP commits **once, cooperatively, at issuance** (it wants the
sale), redemption is pure self-service. The LP **cannot refuse** (there is
no "accept" step to withhold). Therefore the bonded-LP / challenge-protocol
/ no-adjudicator-slash machinery considered for a transferable model is
**unnecessary here** — the no-transfer + pre-signed structure removes the
refusal surface entirely.

The Taproot-Asset "off-chain" wall is sidestepped: **Bitcoin enforces only
that `A` is consumed**; that it is a *burn* (sent to the burn key, supply
reduced) is validated client-side by the TA proof (spec 01). Bitcoin never
introspects TA state. **In v0 (ADR-0005 decision a) the burn key is a
deterministic, NUMS-derived, provably-unspendable sink** (`satusd_crypto::
nums::protocol_sink_script_key`, spec 01 §4), validated client-side by
`satusd-verify` — chosen because tapd's `BurnAsset` cannot compose into the
`redeem_tx` external anchor. Devnet-validated end-to-end (burn ⟺ BTC payout
in one tx): `satusd-rail0/tests/devnet_burn_settle`.

### 3.4 Buyer verification at purchase

The buyer MUST, before paying, verify: the note is funded and
over-collateralised; the pre-signed CETs are valid for the announced oracle
events + buckets and pay `K`; the maturity authorisation (§5) is correct.
Under-funding or a bad CET is caught here, not at redemption.

### 3.5 Why combined (one output), not two-input — and the burn key

An earlier sketch split the note into two inputs: `A` (the holder's plain
SatUSD UTXO) + `Q` (the LP's separate collateral output). It is
**discarded**: the collateral leg would then be a *live* input the LP must
sign at redeem-time (exactly the rail-1 `j4_settle` E2E, whose LP BTC input
is `bitcoind`-signed) — which breaks unilaterality. The **combined** form
(§3.1) puts the collateral *inside the note's own DLC funding output*
(proposal 0001's TA-in-DLC construction), so redemption spends one
oracle-adaptor-locked output and needs no LP input and no LP presence —
truly unilateral. Transferability stays emergently impossible: the
pre-signed CET pays the holder's fixed `K`, useless to anyone else (§8).

**Burn key.** The SatUSD leg pays a provably-unspendable sink. Two
derivations are available: **`protocol_sink`** (a project NUMS key, fixed
per asset family — preconstructable, used in v0) and **`derive_burn_key`**
(tapd's own PrevID-derived `BurnAsset` key — the tapd-standard form, known
only after the input is fixed). Either way the burn is **validated
client-side** by `satusd-verify`, because **atomicity (one tx) rules out
tapd's native `BurnAsset`** — it cannot compose into an external anchor, so
automatic supply-subtree recognition is unreachable here and is left to a
tapd upstream change / the covenant era (spec 01 §4).

## 4. Rolling pre-signature (keeping a note redeemable at a fresh price)

A long-held note must stay redeemable at a *recent* price. The signing
load is three-dimensional and tamed dimension-by-dimension:

| Dimension | Lever | Status |
|---|---|---|
| **price buckets** (per event) | binary digit-prefix compression `S=R+e·P` (2²⁰ → ~16 locks) | **implemented** (`satusd-rail1`) |
| **events** (over time) | coarse **per-block** redemption cadence + a short rolling window; LP continuously pushes fresh CETs | design |
| **positions** (per holder) | per-holder Q now; shared pool in the covenant era (§9) | design |

- **Redemption price = the latest attested event** (per block, not 1 Hz —
  redemption is occasional). The LP keeps the holder supplied with a recent
  CET; the holder redeems unilaterally against it.
- **LP liveness = price freshness, not redemption-ability**: if the LP
  stops pushing, the holder redeems against the last CET held (a slightly
  stale price). They are never stuck.
- Steady-state load ≈ **~16 adaptor sigs per position per block** —
  automatable; horizontal scaling via many LPs.

## 5. Maturity = the LP's committed term

**A note's holding period equals the term its issuing LP committed to at
entry** (the `committed_term` manifest field, spec 02 §2). One parameter sets the holding
period, the LP's capital commitment, and the DLC maturity. Q being
time-locked for the term *is* the commitment — **no separate bond.**

- All notes from one LP mature at that LP's **term-end** (a single, clean
  exit; the buyer sees the maturity date at purchase).
- At maturity, an unredeemed note **auto-settles to BTC at the fair oracle
  price** via a maturity CET the holder **pre-authorised at issuance** —
  self-executing, no rug, no forced loss (it equals a redemption at `T`).
- **Renewal**: before maturity the holder rolls to a fresh-term LP (a
  wallet MAY automate this).
- **Consequence — a maturity spectrum**: the market carries LPs with
  staggered term-ends, so notes come in a range of maturities; buyers pick
  one as they would a bond/CD. Competition sets terms + fees.

## 6. Oracle

The DLC needs **one** oracle key. The decentralised price market (spec 03
§5.5 stake-weighted median) is combined into a single aggregate key via a
**threshold signature (FROST)** (spec 03 §5.7) — avoiding the `C(n,k)`
CET blow-up of
dlcspecs enumerated multi-oracle. The bridge from a *decentralised median*
to *one signature* (a threshold-signing cohort attesting the median) is a
sub-design (§10). **Redemption uses a per-block cadence** (decoupled from
any higher-frequency trading feed).

## 7. Peg & risk (honest)

- **Over-collateralisation + liquidation** (spec 06 CETs) defend the $1
  peg in normal moves; Q is sized for a worst-case price.
- **A catastrophic BTC crash** (faster than liquidation) can break the
  peg; the residual tail risk is borne by **holders** — this is the nature
  of a BTC-backed synthetic dollar (as with DAI/LUSD), stated plainly, not
  papered over. SatUSD is not USDC.
- Below the collateralisation floor, redemption pays Q's balance (a
  shortfall), not a guaranteed $1.

## 8. The no-transfer property (the deliberate trade)

On today's Bitcoin you cannot have **both** free fungibility/transfer
**and** truly-unilateral redemption, because (a) a pre-signed CET has a
fixed payout that a new holder cannot use, and (b) TA state is off-chain,
so a covenant cannot natively introspect a burn. SatUSD chooses
**unilateral redemption over transferability**:

- A note's value *is* its redemption right; transferring the note without
  the right yields the recipient an unbacked shell — so **nobody transfers,
  and SatUSD is non-transferable emergently.**
- To pay, the holder **redeems to BTC and pays BTC** (the payout key `K`
  in §3.1 MAY be set to forward the BTC, e.g. over Lightning — spec 08
  pending).

## 9. The covenant era (future)

Transferability/fungibility **and** shared collateral pools are the **same
covenant capability** (output-constraining / spend-introspection — CTV,
CSFS, CAT, …):

- **Fungible/transferable SatUSD**: a covenant lets the collateral pool be
  claimed by *whoever burns* `$X` of SatUSD — holder-agnostic, so the note
  circulates freely.
- **Shared collateral pool**: a covenant constrains Q's spend so a redeemer
  takes only their share and the remainder stays a valid pool for everyone
  else — removing the per-holder Q multiplier (§4).

Both SHOULD be prototyped on a covenant-enabled signet; mainnet awaits
opcode activation. Until then, the §0–§8 design (per-holder Q,
non-transferable, unilateral) is complete and buildable.

## 10. Open items

1. **`redeem_tx` (combined, §3.5)**: spend the note — one DLC funding output
   holding the SatUSD + collateral (proposal 0001) — to [burn sink, holder,
   LP change] via the oracle-adaptor key-path spend, through tapd's vPSBT
   machinery. Pieces devnet-validated: settle-to-burn
   (`satusd-rail0/tests/devnet_burn_settle`), the oracle-adaptor redeem
   payout (`satusd-vault/tests/vault_redeem_q_to_holder`), and the composed
   deterministic CET (`rail1 j4_settle`); the remaining work is wiring them
   into the single combined-output redeem (its TA leg → burn sink, not a
   transfer).
2. **Maturity CET wiring** + the holder's issuance-time pre-authorisation
   format (spec 06 §5 maturity CET).
3. **Oracle bridge**: decentralised stake-weighted median → one FROST
   aggregate attestation per (per-block) event (spec 03 §5.7 + open
   item 7).
4. **Rolling-window parameters**: cadence, window length, bucket
   granularity vs. signing load; the LP push/refresh protocol.
5. **Lightning support** (spec 08, pending): how far the redeem-to-pay flow
   and cooperative redemption can ride Lightning, and the on-chain DLC
   backstop's relationship to off-chain settlement.
6. **Covenant prototypes** (§9): fungible-claim + shared-pool constructions
   on a covenant signet.
