# SatUSD — Product Requirements Document

- **Version**: v2.0-draft — the pre-covenant **BTC/USD options market**
- **Status**: working contract for the pre-covenant era
- **Scope**: This PRD covers **pre-covenant SatUSD only** (MISSION
  "Two eras"). Post-covenant SatUSD — the fungible, freely-transferable,
  perpetually-pegged, spendable bridge — is **future scope** (§6, §10),
  gated on mainnet covenant opcodes; it gets its own PRD when that era
  opens.
- **Place in the hierarchy**: `MISSION.md` is supreme. This PRD owns
  product scope, actors, journeys, requirements, milestones, and the
  scaffolding ledger. `docs/spec/` owns normative protocol detail —
  spec wins on mechanics, mission wins on intent. ADRs record decisions.
- **Predecessor**: v1.0 (the "unilaterally-redeemable dollar holding /
  redeem-to-pay" framing — superseded by the options-market reframing of
  2026-06; see `docs/research/2026-06-design-journal.md`). Deliberately
  lean: a contract, not an encyclopedia.

## 1. Product definition

Pre-covenant SatUSD is a **self-custodial, KYC-free BTC/USD options
market on Bitcoin L1**. A participant takes a position on their *own*
bitcoin — **short, long, hedge, or lock-in a gain** — without KYC,
without a wrapped token (no wBTC/tBTC), and without ever leaving
self-custodied bitcoin.

**The instrument.** A bitcoin-collateralized **option pair**, settled by
a Discreet Log Contract (DLC) at a **fixed maturity** against an oracle's
BTC/USD attestation. The collateral splits into two legs that always sum
to the locked bitcoin, so **there is no liquidation**:

- **P leg (protective / dollar-stable)** — worth a fixed dollar amount of
  bitcoin at maturity; buying it locks in dollar value / hedges /
  synthetically shorts BTC.
- **N leg (leveraged-long)** — takes bitcoin appreciation above the
  strike; a self-custodial leveraged-long.

**Redemption is unilateral and unstoppable.** Settlement is pre-signed at
issuance; at maturity the holder broadcasts it **alone** (or anyone may,
or a CSV fallback fires) — no issuer signs, no one can freeze or refuse.

**Issued by LPs; a single-hop club secondary market for early exit.**
LPs (market-makers) take the other side and quote both legs. Holders can
sell a not-yet-matured position **once**, peer-to-peer, on an order book
(single-hop, within a pseudonymous membership club). Not a freely
circulating token — that is the covenant era.

**An open-source framework, not a company.** The deliverable is a
framework anyone can deploy to run their own club-market; many small
markets — each anchored by an LP — federate into meaningful volume. The
project seeks to give the Bitcoin community a useful, secure framework,
not to extract rent.

One sentence per audience:

- **For a self-custodial bitcoiner who refuses CEX KYC and wrapped
  tokens**: take a short, hedge, or lock-in on your own bitcoin without
  giving up self-custody or privacy.
- **For an LP / market-maker**: earn the spread making markets in BTC/USD
  options against self-custodial flow — manage your own risk however you
  like (a CEX hedge is one optional, unconnected choice).
- **For a whale / institution**: monetize idle bitcoin by running a
  market, without lending it out or selling it.
- **For an AI agent**: take a dollar-stable or leveraged position you can
  natively hold and settle — no bank, no KYC, machine-verifiable.

## 2. Actors

| Actor | Job to be done | Permission |
|---|---|---|
| **Holder / participant** | take a P-leg (stable / hedge / short) or N-leg (leveraged-long) position; redeem at maturity, unilaterally | club membership (pseudonymous) |
| **LP / market-maker** | issue option pairs against own over-collateralised `Q`; quote + make markets on both legs; pre-sign settlements; recycle `Q`; earn spread; manage own risk (optional, unconnected CEX hedge) | over-collateralised `Q` (2-of-2 with the holder) |
| **Club / market operator** (often = LP) | deploy + run a framework instance; admit members; publish the order book | none (runs their own market) |
| **Oracle** | attest BTC/USD at maturity events (spec 03) | per class; market-priced |
| **Watchtower** | watch for a seller broadcasting a transferred-away (stale) state; punish via the leaked key | none; holder-run or delegated |
| **Broadcaster** | complete a maturity settlement for anyone (permissionless) | none; paid by economics |
| **Founder** | scaffolding roles only (§8); exits | enumerated, with removal criteria |

## 3. User journeys (golden paths)

- **J1 — Join a club.** Pseudonymously register keys/nonces with a market
  and be admitted (no KYC, no real-world identity).
- **J2 — Open a position.** Mint an option pair from an LP and take the
  **P leg** (lock dollar value / hedge / short) or the **N leg**
  (leveraged-long). The wallet verifies that **this position's own `Q`**
  is locked and over-collateralised (P + N = `Q`) and that the LP's
  pre-signed maturity settlement pays the holder the correct amount,
  before showing the position.
- **J3 — Hold & verify.** Any client checks this position's own locked
  collateral, its pre-signed settlement, and its settlement history
  against Bitcoin chain data alone — no server of ours need exist.
- **J4 — Redeem at maturity (unilateral).** At the maturity event the
  holder broadcasts the pre-signed settlement **alone** against the public
  attestation; **or anyone broadcasts it** (permissionless, optional
  bounty); **or**, if no one does, after a CSV the holder takes the
  collateral via a **holder-only** fallback. **No LP, no permission, no
  freeze.** *This path — including the permanently-offline holder — is the
  Phase-1 hard gate (§7).*
- **J5 — Sell on the secondary market (single-hop).** List the position on
  the order book; a club member buys it via an **atomic swap** (buyer pays
  BTC over Lightning/PTLC ⟺ seller arms the buyer's settlement); price is
  discovered freely. The buyer is terminal (holds to maturity or sells
  back to the LP). A watchtower deters the seller from double-dealing.
- **J6 — LP market-make.** Quote both legs; take the other side; manage
  risk privately; recycle `Q` (buy back + cooperatively close, then
  re-issue) to serve sequential holders.
- **J7 — Cooperative early exit.** Holder + LP co-sign a current-price
  unwind (no pre-signed stale prices, so no free-option).
- **J8 — Deploy a market.** Stand up an instance of the open-source
  framework as a club operator / LP.
- **J9 — Challenge.** Detect oracle equivocation (or a stale-state
  broadcast) → submit evidence / use the leaked key → collect the slash.
  The maturity-redemption path itself needs no challenge (no refusal
  surface).

## 4. Functional requirements

Each FR names its normative home, its acceptance check, and its phase.

| ID | Requirement | Spec | Phase | Acceptance |
|---|---|---|---|---|
| FR-1 | TA **option-pair** asset (P/N legs that sum to `Q`) with supply commitments; the one-tx burn⟺claim maturity settlement (spec 07 §3) | 01, 07 | 1 | mint pair + settle round-trip on the dev signet; independent client verifies |
| FR-2 | The **DLC maturity-settlement primitive** — pre-signed, oracle-gated, **unilateral**, two-input (burn position ⟺ claim BTC) | 02 §7, 07 | 1 | a position settles unilaterally with no LP online |
| FR-3 | **2-of-2 MuSig2 collateral `Q`** + MuSig2-**adaptor** maturity CET — so the LP cannot move `Q` before maturity | 06, 07 | 1 | property test: decrypted adaptor verifies under the aggregate `Q`; LP alone cannot spend `Q`; nonce bound to full context, pairwise-distinct |
| FR-4 | **Offline maturity-floor** — a permanently-offline holder is paid at maturity: maturity CET broadcastable by anyone (bounty), or **holder-only CSV** fallback | 07 §5 | 1 | **E2E (HARD GATE): a holder offline since issuance recovers fair-value BTC at maturity, with no LP and no keeper** |
| FR-5 | Oracle daemon: dlcspecs announcements/attestations for **maturity events** (attest only at maturity), digit decomposition, context-bound nonces; transitional | 03 | 1 | independent DLC client consumes its attestations |
| FR-6 | Client verification library: **this position's own locked `Q`** (over-collateralised, P + N = `Q`), option terms, the pre-signed settlement, settlement history — embeddable | 01/02 | 1 | verifies a position is fully backed by its own locked `Q`, with no server of ours |
| FR-7 | **Order book + single-hop transfer**: list/match; atomic swap (PTLC/adaptor) coupling BTC payment ⟺ settlement-arming; LP pre-signs per-member settlements | 02, 07 | 2 | a position sells member→member, atomic, no LP at trade time |
| FR-8 | **Watchtower / stale-state slashing**: detect a transferred-away seller's broadcast; punish via leaked nonce key | 05, 07 | 2 | a planted double-deal is detected and slashed on the dev signet |
| FR-9 | **LP market-making + `Q` recycling**: quote both legs; buy back + cooperative early close; re-issue against recovered `Q` | 02, 06 | 2 | one `Q` serves ≥ 2 sequential holders without on-chain mint-per-holder |
| FR-10 | **Cooperative early exit**: holder + LP co-sign a current-price unwind | 07 | 2 | a position unwinds early at the live co-signed price, no stale CET |
| FR-11 | **Membership / club enrollment**: pseudonymous nonce registration; the per-member pre-signing set | 02 | 2 | a new member is admitted and can be a transfer counterparty |
| FR-12 | **Deployable open-source framework**: a club operator stands up a market (LP daemon, order book, oracle hookup, holder/MM wallets) from docs | — | 3 | an independent team runs a market on the dev signet from the repo alone |

## 5. Non-functional requirements

- **NFR-1 Holder safety is liveness-free.** A holder's eventual payout
  (maturity floor, FR-4) depends on **no one's** liveness — not the LP's,
  not a keeper's. Only *fresh-price early exit* needs counterparties.
- **NFR-2 Verifiability.** No protocol claim depends on a server we run.
  Kill all our infrastructure; holders still verify and redeem with only
  a Bitcoin node.
- **NFR-3 Cross-language correctness.** All consensus-relevant encodings
  pinned by Rust = TS vectors; `make verify` green is a merge gate.
- **NFR-4 No permission layer on the holder.** No KYC, no freeze, no admin
  key over the holder's self-custodied bitcoin or their unilateral
  redemption. (Membership is a pseudonymous club; an LP's optional CEX
  hedge is its own private, unconnected matter.)
- **NFR-5 LP custody is trustless (Phase 1).** Via 2-of-2 MuSig2 `Q`
  (FR-3), the LP cannot move, seize, or freeze a holder's collateral; it
  can at most decline to *open* a new position or *facilitate* a transfer.
- **NFR-6 Trading is off-chain / instant.** Entering a position (mint) and
  exiting (maturity redeem) are on-chain (infrequent, block-paced); the
  **secondary-market trade is off-chain and seconds-latency** (PTLC +
  secret-reveal; CETs pre-signed at mint, not regenerated per trade).
- **NFR-7 No free-option.** Maturity-only settlement ⇒ a single event,
  single price, no accumulable stale authorizations (spec 07).
- **NFR-8 Cost posture.** Founder-funded until LP/market economics cover
  ops; listed in the scaffolding ledger.

## 6. Out of scope (pre-covenant)

Per MISSION and the two-era split:

- **All post-covenant features** — fungibility; free/permissionless
  transfer; multi-hop transfer; perpetual peg (anytime current-price
  redemption); a spendable circulating token; a no-club permissionless
  market; the price-signal-migration / self-referencing oracle as a
  present goal. These return in the covenant era (own PRD).
- **Perpetual-style synthetics** (10101's path) — coordinator-in-the-loop,
  liquidation, DLC channels. Deliberately not pursued (design journal).
- yield on holdings; fiat reserves; KYC/compliance; institutional custody
  (permanent non-goals).
- commercialization / rent extraction; governance token; legal entity
  (permanent non-goals per MISSION).

## 7. Milestones — metric-gated, no dates

Phases are recognized by checks passing, not calendars. The pre-covenant
work is built on a **custom signet with covenant opcodes activated but
unused** (MISSION "How both eras are built").

**M-A — The redemption primitive runs (regtest).** ✅ 2026-06-10.
The two-input DLC burn⟺claim settlement, oracle daemon, client
verification, and Rail-0/Rail-1 references are E2E green on regtest —
the cryptographic core the option settlement reuses.

**M-1 — The trustless core (Phase 1, dev signet).**
☑ FR-1 option-pair mint (mint + settle round-trip, devnet-validated 2026-06-17) ·
☑ FR-2 unilateral maturity settlement (two-input burn⟺claim, both min(S,x) regimes, devnet-validated 2026-06-17) ·
☑ FR-3 **2-of-2 MuSig2 `Q` + MuSig2-adaptor CET** (devnet-validated 2026-06-17) ·
☑ **FR-4 offline maturity-floor E2E — the hard gate** (offline-since-issuance
holder recovers fair-value BTC with no LP, no keeper; devnet-validated 2026-06-17) ·
☐ FR-5 maturity-event oracle ·
☑ FR-6 client verification — a position is backed by its own locked `Q`, P+N=`Q`, CET armed for the announced event (`satusd-verify::position`, 2026-06-17).

**M-2 — The market (Phase 2, dev signet).**
☐ FR-7 order book + single-hop atomic-swap transfer ·
☐ FR-8 watchtower / stale-state slashing ·
☐ FR-9 LP market-making + `Q` recycling · ☐ FR-10 cooperative early exit ·
☐ FR-11 club membership.

**M-3 — The framework (Phase 3).**
☐ FR-12 an independent team deploys a market from the repo ·
☐ ≥ 1 external club / community running · ☐ ≥ 1 external LP making markets.

**M-∞ — Post-covenant (future, gated on mainnet covenants).**
The bridge — its own PRD when the era opens.

## 8. Scaffolding ledger

Transitional compromises, enumerated with removal criteria; ships with
every release.

| Scaffold | Why it exists | Removal criterion |
|---|---|---|
| Custom covenant-signet testbed | mainnet has no covenants; we develop both eras here | n/a (a testbed, not a shipped control); post-covenant code gates on mainnet activation |
| Founder LP / founder-run market | LP cold-start; no external market-makers yet | ≥ 1 external LP making markets (M-3) |
| Founder-run single oracle | dlcspecs oracle market is empty today | ≥ 1 independent oracle class with market share |
| Single founder-run club | bootstrapping; no external operators | ≥ 1 external club deployed from the repo (M-3) |
| TA group-key custody (issuance authority) | grouped-asset issuance needs a signature; threshold/covenant issuance not built | FROST k-of-n group key, or covenant-gated issuance |
| Reference BTC/USD marker | external marker until an internal market earns authority (covenant-era goal) | covenant-era self-referencing criterion |
| Founder as sole maintainer | solo + AI build phase | ≥ 2 maintainers with release capability |
| Manual / early slash execution | dispute automation staged | automated stale-state + oracle-equivocation slashing live |

## 9. Top risks

1. **Demand-side / cold start (highest).** Technical capability ≠ market
   demand — **10101 built a similar self-custodial synthetic and died of
   no traction.** The first club / community is unvalidated. Mitigation:
   target the specific underserved segment (self-custodial, anti-KYC,
   anti-wrapping bitcoiners who want to hedge/short/lock-in); ship as a
   free open framework so usefulness, not growth-or-die, is the bar.
2. **Offline maturity-floor never E2E'd.** Invariant-4's only true
   guarantee is untested (spec 07 §10). Mitigation: it is the Phase-1
   hard gate (FR-4) — proven before anything is built on top.
3. **MuSig2-adaptor is unwritten, sensitive crypto.** `musig2` may not
   expose adaptor injection; worst case a BIP-327 adaptor must be written.
   Until FR-3 lands, "LP can't move `Q`" is paper. Mitigation: Phase 1,
   with property tests + strict nonce discipline.
4. **Liquidity fragmentation.** Small clubs are thin. Mitigation: each
   club needs a committed market-making LP as its liquidity core.
5. **PTLC / Lightning maturity** for atomic-swap trades. Fallback:
   on-chain BTC payment (~1 block) or an operator-escrowed match.
6. **Stale-state griefing** in the secondary market (watchtower load).
7. **Founder concentration** — bus factor + legal exposure; mitigated by
   the exit ledger and the open-source framework goal.

## 10. Open product questions

1. **First target club / community** (demand validation — the top risk).
2. **Order-book transport / matching** — decentralized vs operator-run
   (Nostr a candidate).
3. **Watchtower design** — holder-run vs a delegated service; incentives.
4. **Wallet strategy** — minimal reference wallet (CLI + thin web) vs
   integrate an existing TA wallet.
5. **Custom covenant-signet** — Bitcoin Inquisition signet vs roll our own.
6. **Series standardization** for within-series fungibility — the strike ×
   maturity grid (covenant-era prerequisite, but the pre-covenant order
   book benefits from it too).
