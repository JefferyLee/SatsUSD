# ADR-0005: SatUSD is a no-transfer, unilaterally-redeemable holding — the redemption-bearing note is the core object

- **Date**: 2026-06-16
- **Status**: accepted
- **Authority**: `docs/MISSION.md` v2 via ADR-0001. **Supersedes ADR-0003**
  (settle-to-LP / recirculation). **Re-scopes ADR-0004** (vault →
  LP-supply-side). **Defers** spec 04 reserve + ADR-0002 capacity to the
  covenant era. Design home: `docs/spec/07-redemption-notes.md` +
  `docs/spec/08-lightning.md`.
- **Author**: Jeffery (decisions) + AI agent (drafting)

## Context

A first-principles design session (2026-06-16) settled what SatUSD
fundamentally *is*. The prior document set — ADR-0003, ADR-0004, spec 02
(rail standard), spec 04 (reserve), and the PRD journeys — was written
under an implicit **transferable / circulating** model: SatUSD as a
conversion-rail asset that a holder could settle *to an LP* (a P2P trade),
which the LP could **recirculate** (market-making), all backed by a
**common reserve** sized for circulating supply.

That model carried two unsolved burdens: a transferable claim needs either
a covenant (absent on Bitcoin today) or a bonded-LP refusal surface plus a
challenge/slash protocol; and a circulating supply needs a shared reserve
with capacity accounting. The session found a cleaner foundation by
**dropping transferability**:

> Every SatUSD note carries a **pre-signed, oracle-gated, unilaterally-
> broadcastable DLC redemption**. Redemption is **one Bitcoin transaction
> with the note as a required input** (`redeem_tx`: inputs [the LP's
> collateral `Q` + the note `A`], outputs [note→burn key, `X/P` BTC→holder,
> change→LP]). The LP pre-signs the `Q`-spend at issuance; the public oracle
> attestation completes it; the holder broadcasts alone.

Because the LP commits **once, cooperatively, at issuance** (it wants the
sale), there is no "accept" step to withhold — the LP **cannot refuse**.
The bonded-LP / challenge / adjudicator machinery built for a transferable
model becomes unnecessary (spec 07 §3.3). The trade is deliberate: on
covenant-less Bitcoin you cannot have *both* free transfer *and* truly-
unilateral redemption, so SatUSD chooses unilateral redemption and is
**emergently non-transferable** (a note minus its bound redemption right is
an unbacked shell, so nobody transfers).

## Decision

1. **SatUSD is a BTC-backed, self-custodied, unilaterally-redeemable dollar
   *holding* — not a circulating payment medium** (spec 07 §0). It is
   **spendable but not transferable**: a wallet "spends" via *redeem-to-pay*
   (redeem note→BTC, pay BTC; the payer's UX is "I spent $X"), but SatUSD
   never changes hands as SatUSD — the counterparty receives BTC. Three
   layers: **SatUSD = unit of account + spend-trigger; BTC = settlement
   medium.** "spendable / pay" language is correct and kept; "transferable /
   P2P / circulating / handed to another holder" is retired.

2. **Three structural forks settled (2026-06-16):**
   - **(a) Rail-0 / swap = issuance only.** A rail swap survives solely as
     *buy a note from an LP* (BTC → a fresh note with its bound redemption
     right). **settle-to-LP recirculation and swap-style redemption are
     retired.** Redemption is the spec 07 unilateral one-tx DLC.
   - **(b) spec 02 is deep-restructured around the note.** "Rail" is demoted
     from *conversion channel* to **LP-issuance channel + BTC-leg + price-
     discovery**. The four properties (S1 unilateral exit, S2 conservation,
     S3 observer verifiability, L1 bought liveness) remain cross-cutting
     invariants. The QUOTE→LOCK→SETTLE state machine collapses for
     redemption to a **single unilateral broadcast** (CETs armed at
     issuance, not a fresh per-redemption lock).
   - **(c) The common reserve (spec 04) + capacity (ADR-0002) defer to the
     covenant era.** v0 backing is **per-note over-collateralised `Q`**
     (the LP's own locked collateral). Shared collateral pools and
     fungibility are the *same* covenant capability (spec 07 §9), unlocked
     together; the reserve/reimbursement/`lp_stake`-capacity model sleeps
     until then.

3. **Supersedes ADR-0003.** A SETTLE whose TA leg lands on the LP's script
   key (a "pure P2P trade" the LP may "recirculate") is **no longer
   conforming**. The only conforming disposition of a note is
   **burn-on-redeem** — the note is a required input sent to the burn key in
   the same tx that pays the holder BTC. ADR-0003's "burn attaches to
   reimbursement" rule is moot in v0 (there is no reserve reimbursement).

4. **Re-scopes ADR-0004.** The **vault is a collateral primitive usable by
   both a CDP self-minter and an LP-issuer.** The user-facing object is a
   redemption-bearing note **bought from an LP**; the LP MAY source its
   collateral `Q` by opening a vault. "铸造即开仓" describes the LP/CDP
   *supply side*, not the end holder's acquisition. The scaffolding-ledger
   removal criterion is restated against **held / outstanding** SatUSD (there
   is no "circulating" SatUSD).

5. **Maturity = the LP's committed term** — a new `RailManifest` field
   (`committed_term`, spec 02 §2); at term-end an unredeemed note auto-
   settles to BTC at the fair oracle price via a maturity CET the holder
   pre-authorised at issuance. Renewal = roll to a fresh-term LP.

## Consequences

- The decision log now records the project's **largest pivot**, which until
  now lived only in spec 07/08 (self-described as "design/aspirational").
- A **doc-reconciliation pass** follows this ADR: MISSION (one phrase:
  "quoted in SatUSD, settles in BTC"), PRD (§1, §3 J1/J3/J7, §4 FR-1/FR-3,
  §8, + a maturity requirement), spec 01 (name the LP-issuer), spec 02
  (the restructure), spec 03 (FROST aggregation §5.7 + per-block cadence),
  spec 04 (covenant-defer scope note), spec 06 (vault reframe +
  `crash_adaptor_point` naming), spec 07 (fix four dangling cross-refs), and
  the **public** `0001-cover-post` ("transferable bearer asset" → the
  holding framing; check the pitch repo for the same wording).
- **No new trust.** Redemption is self-service; no bond/challenge/
  adjudicator on the redemption path. **spec 05** dispute machinery stays
  scoped to oracle equivocation + rail over-cap (it never covered
  redemption) — no conflict.
- **Code is unaffected.** The spec 02 restructure is framing-only: the
  `satusd-rail0`/`satusd-rail1` DLC adaptor-signature mechanics and the four
  properties are preserved, only reorganised around the note and stripped of
  the transfer/recirculation/reserve coupling.
- **ADR-0002's capacity formula is preserved, dormant.** It caps reserve
  draw; with no reserve draw in v0 it has nothing to cap. It returns with the
  covenant-era shared pool.

## Alternatives considered

- **Keep transferability (the prior model).** Requires a covenant (not yet
  on Bitcoin) or a bonded-LP refusal surface + challenge protocol, and
  reintroduces the binding/fungibility problem. **Deferred** to the covenant
  era (spec 07 §9), not abandoned.
- **Keep settle-to-LP recirculation as a convenience path** (the "keep both
  swap directions" fork). Leaves a transfer surface that contradicts the
  no-transfer property and recreates the unbacked-shell problem. **Rejected.**
- **Keep the common reserve as a parallel v0 layer** alongside per-note `Q`.
  Two collateral-accounting systems for no transfer benefit while SatUSD does
  not circulate. **Rejected** in favour of deferring the reserve whole.
- **Restructure spec 02 minimally (re-scope only).** Cleaner end-state was
  judged worth the heavier rewrite (decision 2b); the four properties and
  DLC mechanics survive either way, so the risk is bounded.
