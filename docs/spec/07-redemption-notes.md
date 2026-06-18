# SatUSD — the BTC/USD option and its unilateral maturity settlement

- **Spec**: 07
- **Version**: 0.2-draft (reconceived as an option per ADR-0006/0007;
  supersedes the 0.1 "anytime-redemption note" framing)
- **Authority**: `docs/MISSION.md` v3 ("Two eras") via ADR-0001,
  ADR-0006 (the option-market reconception), ADR-0007 (the covenant wall
  / two-era split)
- **Depends on**: 00 (conventions), 01 (asset/burn), 02 (rail standard),
  03 (oracle), 06 (vault funding output + CETs)
- **Status**: design — pre-covenant only. Normative language is
  aspirational until pinned by test vectors + a signet prototype.
  Research: `docs/research/2026-06-design-journal.md`,
  `docs/research/2026-06-landscape-and-bitvm-research.md`.

The key words MUST, MUST NOT, SHOULD, MAY are RFC 2119.

## 0. What SatUSD is — and is not (pre-covenant)

Pre-covenant SatUSD is a **fixed-maturity, BTC-collateralized option**,
settled by a DLC at maturity against an oracle BTC/USD attestation. The
product is a **self-custodial, KYC-free BTC/USD options market** — *not*
a circulating dollar. Defining properties:

> Locked collateral `Q` splits into two legs that always sum to `Q`, so
> **there is no liquidation**. The **P leg** (protective / dollar-stable)
> is worth a fixed dollar amount of BTC at maturity — buying it locks in
> dollar value, hedges, or synthetically shorts BTC. The **N leg**
> (leveraged-long) takes the BTC appreciation above the strike. At
> maturity the holder settles **unilaterally**: a pre-signed,
> oracle-gated DLC the holder broadcasts alone — no issuer signs, no one
> can refuse or freeze.

This differs from every other dollar instrument: USDC's issuer can
freeze; DAI depends on a live protocol; a CDP gets liquidated; Ethena's
holder is exposed to an exchange hedge. SatUSD's holder holds a
self-custodial option backed by **its own locked BTC**, settled by a DLC
the holder broadcasts alone.

**Why an option, and why maturity-only.** "Anytime redemption at the
*current* price" cannot be built trustlessly on today's Bitcoin without a
**free-option** exploit (a holder accumulates pre-signed claims at many
prices and cherry-picks); see ADR-0007 for the impossibility result.
Maturity-only settlement has a **single** oracle event, one price, no
rolling, no accumulable stale authorizations — so **free-option = 0**.
Fungibility, free transfer, and a perpetual (anytime current-price) peg
are deferred to the covenant era (§11).

## 1. Roles & lifecycle

| Role | Definition |
|---|---|
| **Holder** | holds a P-leg (stable / hedge / short) or an N-leg (leveraged-long) position + its unilateral maturity settlement right |
| **LP / market-maker** | locks `Q`, mints the option pair, pre-signs the maturity CET, takes the other side and makes markets; the BTC-risk bearer (may hedge privately — §0) |
| **Oracle** | attests BTC/USD at the maturity event per spec 03 (one aggregate key, §8) |
| **Broadcaster** | anyone may complete a maturity settlement (permissionless) |
| **Watchtower** | (secondary market) watches for a transferred-away seller broadcasting a stale state; punishes via the leaked nonce key (§7) |

```
issue   (buy from an LP; LP locks Q (2-of-2 MuSig2, §5), splits it into
         the P/N legs, pre-signs the maturity CET; the buyer holds the note)
  → hold (a dollar-stable or leveraged-long position to maturity)
  → settle at maturity   (one tx: burn the note ⟺ claim the option payout; §4)
        · unilateral; or anyone broadcasts; or holder-only CSV fallback (§6)
  → (optional) sell once on the secondary market before maturity (§7)
  → (optional) cooperative early exit with the LP (§7)
  → renew  (buy a fresh-term position at maturity / ladder)
```

## 2. Issuance — minting the option pair

SatUSD is **minted on demand by an LP at the point of sale**; supply
expands when bought, contracts when settled, always backed by locked BTC.
The buyer pays BTC; the LP locks **over-collateralised BTC in a DLC
funding output `Q`** (spec 06 §2; key-path a **2-of-2 MuSig2(LP, holder)**
— §5), splits the economic exposure into the **P leg** (the SatUSD note
`A`, a Taproot-Asset UTXO the buyer holds) and the **N leg** (the
leveraged-long residual; in v0 held by the LP, tradeable as the market
matures), and **pre-signs `Q`'s oracle-adaptor maturity CET** (§4.2). The
TA×DLC composition follows `docs/proposals/0001`.

**Per-position backing.** Each option pair is backed by **its own locked
`Q`** (`P + N = Q`). There is no shared pool and no global-solvency trust
— a holder verifies *their own* position's `Q` (§4.4). (A shared pool is
covenant-era, §11.)

**Risk attribution.** A BTC↔SatUSD mint does not create risk; it
*transfers BTC exposure*. The P-leg buyer goes flat-BTC / long-USD; the
N-leg / LP is long BTC + earns the spread — a position taken willingly.
How the LP manages that exposure (directional, on-chain hedge, an
optional CEX delta-neutral hedge with no formal connection to the
framework, or unhedged) is the LP's own affair (§0).

## 3. The two legs and no liquidation

Let `S` be the strike and `x` the BTC/USD price the oracle attests at
maturity `M`. The locked `Q` (one BTC, w.l.o.g.) is split:

- **P leg** receives `min(1, S/x)` BTC — i.e. value `min(x, S)` in USD: a
  fixed `S` dollars while `x ≥ S` (a synthetic dollar), tracking BTC down
  below `S`.
- **N leg** receives `max(0, 1 − S/x)` BTC — the appreciation above `S`
  (a leveraged-long / long-call payoff).

`P + N ≡ 1` BTC at every price, so **the pair can never be
under-collateralised against itself and there is nothing to liquidate.**
(This is the Vitalik-style P/N split, realised on Bitcoin via a DLC.) The
P leg is the validated unilateral-settlement object (§4); a separately
held, separately tradeable N leg gets a symmetric claim path as the
market matures (PRD M-2).

## 4. The maturity settlement — the binding construction (core)

The hard problem: bind a pre-signed settlement right to the note such
that (1) settlement is unilateral, (2) burn ⟺ claim is atomic, (3) no
double-claim. Secret-revelation schemes fail (anything the holder can
compute, they can compute without burning). The binding is enforced by
**a single Bitcoin transaction with the note as a required input** — the
same `redeem_tx` already devnet-validated, now bound to the **maturity
event** rather than a rolling one.

### 4.1 The settlement transaction

```
settle_tx (maturity):
  inputs:  [ Q  (the option's over-collateralised, pure-BTC DLC output)
             A  (the holder's P-leg SatUSD note UTXO) ]
  outputs: [ A's SatUSD → burn sink   (spec 01 §4 — the note is destroyed)
             min(S, x) worth of BTC → K   (the P-leg holder's payout key)
             change BTC → LP / N leg ]
```

- **Atomicity is structural, not cryptographic**: `A` is a *required
  input* sent to the burn sink, in the *same* tx that claims from `Q`.
  You cannot claim without burning; you cannot double-claim (`A` is spent
  once).
- **Unilateral**: the holder signs `A`; `Q` is spent by the LP's
  **issuance-time oracle-adaptor pre-sign**, completed by the public
  oracle attestation — **no LP signature and no LP presence at maturity**.
- The P-leg payout is `min(S, x)`; the N-leg / LP keeps the change. Below
  the over-collateralisation floor the P payout is capped at `Q`'s balance
  (a shortfall — §9).

### 4.2 Authorisation

- **Q leg**: the LP **pre-signs at issuance** the maturity CETs spending
  `Q`, one per price bucket of the **single maturity event**, as **adaptor
  signatures** locked to that bucket's oracle anticipation point (proposal
  0001 §3.2; spec 06; spec 03 §3). Each commits the `settle_tx` structure
  (a `SIGHASH_DEFAULT` key-path spend of `Q`). Under 2-of-2 MuSig2 `Q`
  (§5) the LP's contribution is a MuSig2 **adaptor** partial; the holder
  completes with their own partial.
- **A leg**: the holder signs `A` at maturity (authorising the burn).
- **Completion**: the **public maturity attestation** decrypts the
  matching bucket and completes the adaptor; the holder — or any
  broadcaster, since the payout is fixed to `K` — broadcasts. No LP at
  maturity, no adjudicator, no covenant.

### 4.3 Why this is sound (no bond / challenge needed)

The LP commits **once, at issuance** (it wants the sale); settlement is
pure self-service. The LP **cannot refuse** (there is no "accept" step to
withhold), so no bonded-LP / challenge-protocol machinery is needed —
the pre-signed structure removes the refusal surface. (The *custody*
surface — the LP moving `Q` before maturity — is closed by §5's MuSig2,
unlike the v0 single-key gap.)

The Taproot-Asset "off-chain" wall is sidestepped: **Bitcoin enforces
only that `A` is consumed**; that it is a burn (supply reduced) is
validated client-side by the TA proof (spec 01). The burn key is a
provably-unspendable, NUMS-derived sink (`satusd_crypto::nums::
protocol_sink_script_key`, spec 01 §4), validated client-side by
`satusd-verify` — because atomicity (one tx) rules out tapd's native
`BurnAsset` (it cannot compose into an external anchor; left to a tapd
upstream change / the covenant era). Devnet-validated burn ⟺ payout in
one tx: `satusd-rail0/tests/devnet_burn_settle`, and the full two-input
form `satusd-rail1/tests/devnet_settle::redeem_two_input`.

### 4.4 Buyer verification at purchase

The buyer MUST, before paying, verify: **this position's own `Q`** is
funded and over-collateralised (`P + N = Q`); the pre-signed maturity
CETs are valid for the announced maturity event + buckets and pay `K`;
the maturity event/strike are as quoted. Under-funding or a bad CET is
caught here, not at maturity.

## 5. Collateral custody — 2-of-2 MuSig2 `Q` (Phase 1)

`Q`'s key-path is a **2-of-2 MuSig2(LP, holder)** output, with a
script-path **holder-only CSV** fallback (§6). Consequence: **the LP
cannot move, seize, or freeze `Q` before maturity** — the only pre-maturity
spend is a settlement both parties' partials authorise (the LP's via its
pre-signed adaptor). This closes the v0 single-key custody gap (where a
single LP-held key let a malicious LP pre-empt the collateral). Settlement
remains unilateral: the holder holds the LP's adaptor partial + their own
partial + the attestation, and broadcasts alone.

> Engineering note: the maturity CET needs a **MuSig2-*adaptor*** (the
> aggregate-nonce adaptor + even-Y handling), not just a MuSig2 keyspend.
> **Built and devnet-validated (FR-3, 2026-06-17).** `satusd-vault`'s
> `musig::cosign_keyspend_adaptor` produces the 2-of-2 adaptor (both
> partials, `receive_signature` verifying the counterparty's before
> aggregation) and `adapt_keyspend` decrypts it with the published oracle
> scalar; the `musig2` crate handles the even-Y dance internally. A unit
> test confirms the adapted joint signature verifies under the aggregate
> `Q` with the project's *independent* secp256k1 (and the wrong scalar
> does not), and `devnet_vault::vault_musig2_maturity_settle` confirms it
> on-chain: a 2-of-2 `Q` is funded, the maturity CET is co-signed at
> issuance, and at maturity the public attestation alone adapts it into a
> valid key-path spend. "The LP cannot move `Q`" is no longer paper.

## 6. Maturity, the offline floor, and free-option = 0

**A position's holding period is the LP's committed term** (the
`committed_term` manifest field, spec 02 §2): one parameter sets the
holding period, the LP's capital commitment, and the DLC maturity. `Q`
time-locked for the term *is* the commitment — no separate bond. Notes
from one LP mature at its term-end; the market carries staggered terms,
so positions come in a **maturity ladder** (buyers pick a term as they
would a bond/CD, and ladder for rolling liquidity). **Renewal** = buy a
fresh-term position at maturity (a wallet MAY automate this).

**free-option = 0.** Settlement is bound to the **single** maturity event
— one price, no rolling, no menu of historical prices to cherry-pick. (A
small designed convexity from bucket granularity is priced toward the
holder's disadvantage and absorbed by over-collateralisation; §9.)

**The offline floor (the Phase-1 hard gate — PRD FR-4).** A holder
**offline since issuance** is still paid at maturity, with **no
dependence on anyone's liveness**:

1. `Q`'s outpoint is fixed for the whole term (no rolling moves it).
2. At maturity the attestation is public → the maturity CET's adaptor is
   decryptable → **anyone may broadcast it** (optionally for a micro
   bounty), paying the holder's `K`.
3. **Even if no one does**, after a CSV the holder spends `Q` via the
   **holder-only** script-path and recovers `Q`'s BTC alone — no LP, no
   keeper, no oracle-at-that-moment.

This is the **only** unconditional guarantee in the system and **has
never been E2E tested** (an open item, §12) — proving it is the Phase-1
hard gate.

## 7. The secondary market — single-hop transfer (Phase 2)

A position MAY be sold **once**, peer-to-peer, before maturity, on an
order book within a **pseudonymous membership club** (the issuing LP
pre-signs settlements payable to each enrolled member):

- **Atomic, off-chain trade**: the buyer pays BTC over Lightning (a PTLC),
  and claiming that payment reveals the seller's transfer secret that
  **arms the buyer's pre-signed settlement** — "buyer pays ⟺ buyer gets a
  valid settlement", no escrow, no LP at trade time. CETs are pre-signed
  at mint, so a trade is a secret-reveal + a Lightning payment, not a
  re-sign (the trade is seconds-latency; only mint/maturity are on-chain).
- **Single-hop / terminal buyer**: the buyer holds to maturity or sells
  back to the LP; they cannot re-transfer onward (no second-hop pre-sign).
  Open, multi-hop, fungible circulation is covenant-era (§11).
- **No double-deal**: the seller's settlement and the transfer use the
  seller's **single-use nonce**; doing both leaks the seller's key
  (equivocation), letting the buyer / a **watchtower** punish — the only
  trustless on-chain enforcement primitive on Bitcoin today.
- **LP liquidity & `Q` recycling**: the LP MAY make markets on both legs;
  buying a position back (with the seller co-signing an early close)
  recovers `Q` early to re-issue — one `Q` serving sequential holders.
- **Cooperative early exit**: a holder + LP MAY co-sign a current-price
  unwind (a fresh co-signed price, so no free-option).

Pricing is free-market (the order book); a position is worth its
mark-to-market (≈ face for a deep-in-the-money P leg). The detailed
order-book / transfer mechanism is its own buildout (PRD M-2).

## 8. Oracle

The DLC needs **one** oracle key. The decentralised price market (spec 03
§5.5 stake-weighted median) is combined into a single aggregate key via a
**threshold signature (FROST)** (spec 03 §5.7) — avoiding the `C(n,k)`
CET blow-up of dlcspecs enumerated multi-oracle. Settlement uses a
**single maturity event**; the oracle **attests only at maturity** (not a
real-time feed) — and SHOULD NOT publish the maturity price early (else an
LP could front-run a holder), a trust assumption to disclose alongside the
oracle's own (spec 03; timelock-encrypting the maturity price is a
candidate hardening). The bridge from a decentralised median to one
signature is a sub-design (§12).

## 9. Peg & risk (honest)

- **Over-collateralisation** defends the `S`-dollar peg of the P leg in
  normal moves; `Q` is sized for a worst-case `x`. Deep-in-the-money
  (strike well below spot) ⇒ the P leg is robustly ≈ `S` dollars and the
  LP's hedge is near-static.
- **A catastrophic BTC crash below the floor** breaks the peg; the
  residual tail is borne by the **holder** — the nature of a BTC-backed
  synthetic dollar (DAI/LUSD), stated plainly. SatUSD is not USDC. Below
  the floor, the P payout is `Q`'s balance, not a guaranteed `S`.
- **No liquidation** (§3): there is no forced sell-off and no
  real-time-oracle dependency — the crash risk is a settlement-price risk,
  not a liquidation-cascade risk.
- **No free-option** (§6) and **trustless custody** (§5, once MuSig2
  lands) — the two gaps the earlier rolling/single-key design carried.

## 10. Paths deliberately NOT taken

Recorded so they are not re-proposed as pre-covenant main paths (ADR-0006/
0007; research):

- **Anytime current-price redemption via rolling pre-signature** — dropped.
  It is the source of the free-option (no signature expiry on Bitcoin);
  invalidating stale authorizations needs a covenant or a channel
  (penalty + watchtower + a weaker, multi-tx unilateral backstop). Our
  single-tx maturity settlement is strictly stronger on the unilateral
  axis.
- **DLC channels / DLC factories** — parked. The revocation+penalty model
  weakens the unilateral backstop and needs a watchtower; factories also
  put an offline member's funds at risk. (Detail in the 2026-06 research.)
- **A 10101-style perpetual** — rejected. It needs an always-in-the-loop
  coordinator, liquidation, funding, and DLC channels (operationally fatal
  for 10101). A fixed-maturity option avoids all four.
- **BitVM-emulated covenants for the core** — rejected: they reintroduce a
  signer committee / operator / challenge windows (no-issuer / unilateral
  violations); only their minimal primitives are of interest.

## 11. The covenant era (future)

Transferability/fungibility, a perpetual (anytime current-price) peg, and
shared collateral pools are the **same covenant capability**
(`CHECKSIGFROMSTACK` to verify the oracle in script + amount/output
introspection such as `OP_CHECKCONTRACTVERIFY`):

- **Fungible / freely-transferable SatUSD**: a covenant lets the payout
  re-bind to *whoever holds the asset* and the price track the *latest*
  attestation — holder-agnostic, freely circulating, free-option-free
  without maturity.
- **Shared collateral pool**: a covenant constrains `Q`'s spend so a
  redeemer takes only their share and the remainder stays a valid pool —
  removing the per-position `Q`.

Both SHOULD be prototyped on the covenant-enabled signet (MISSION "How
both eras are built"); mainnet awaits opcode activation. Until then, the
§0–§10 design is complete and buildable on today's rules.

## 12. Open items

1. ~~**The offline maturity-floor E2E — the Phase-1 HARD GATE (§6).**~~
   **✓ DONE (2026-06-17).** A holder offline since issuance recovers
   fair-value BTC at maturity both ways — anyone-broadcasts and holder-only
   CSV — with no LP and no keeper (`rail1` `devnet_settle::offline_maturity_floor`
   + `offline_maturity_floor_csv_fallback`).
2. ~~**MuSig2 `Q` + MuSig2-adaptor maturity CET (§5)**~~ **✓ DONE
   (2026-06-17).** Built (`vault` `musig::cosign_keyspend_adaptor` /
   `adapt_keyspend`) with a unit test under the project's independent
   secp256k1 and the on-chain `devnet_vault::vault_musig2_maturity_settle`;
   custody is no longer v0-single-key. (Strict nonce discipline + the
   rolling/pre-authorisation wiring of item 3 still to harden.)
3. **Maturity CET wiring** + the holder's issuance-time pre-authorisation
   format (spec 06 §5).
4. **Oracle bridge**: decentralised stake-weighted median → one FROST
   aggregate attestation per maturity event (spec 03 §5.7); and the
   "do-not-publish-early" hardening (§8; timelock encryption candidate).
5. **Secondary-market mechanism (§7)** — order-book transport, the
   single-hop atomic-swap (PTLC) construction, watchtower design,
   membership enrollment. Phase 2 (PRD M-2).
6. **Covenant prototypes (§11)** — fungible-claim + shared-pool on the
   covenant signet. Phase ∞.
