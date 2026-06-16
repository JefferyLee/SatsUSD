# SatUSD — Product Requirements Document

- **Version**: v1.0-draft
- **Status**: working contract for the rebuilt project
- **Place in the hierarchy**: `MISSION.md` is supreme. This PRD owns
  product scope, actors, journeys, requirements, milestones, and the
  scaffolding ledger. `docs/spec/` owns normative protocol detail —
  where this document and a spec disagree on protocol mechanics, the
  spec wins; where either disagrees with the mission, the mission
  wins. ADRs record the decisions that shaped both.
- **Predecessor**: the v5.1 PRD (archived, superseded). This document
  is deliberately lean; it is a contract, not an encyclopedia.

## 1. Product definition

SatUSD is a bitcoin-collateralized, dollar-denominated Taproot Asset
on Bitcoin L1 — a **self-custodied, unilaterally-redeemable dollar
holding**. Every note carries a pre-signed, oracle-gated DLC
redemption the holder broadcasts alone (spec 07): holding requires no
permission; redeeming requires no trusted intermediary and no
counterparty's consent; verifying requires only Bitcoin. Notes are
**issued on demand by competing LPs** and redeemed unilaterally — they
are not handed peer-to-peer.

**Spendable, not transferable (the doctrine — ADR-0005).** A wallet
*spends* SatUSD by *redeem-to-pay*: it redeems the note to BTC and pays
BTC, so the payer experiences spending dollars while the counterparty
receives BTC. SatUSD itself never changes hands as SatUSD. Three
layers: **SatUSD is the unit of account + the spend trigger; BTC is the
settlement medium.** ("spendable / pay" is correct; "transferable /
circulating / P2P" is not — transferability returns in the covenant
era, spec 07 §9.)

One sentence per audience:

- **For a person under capital controls or debanking**: dollars that
  no company and no government can freeze, in your own keys.
- **For a bitcoiner**: spendable dollar stability without leaving
  Bitcoin L1 or touching a fiat reserve.
- **For an LP**: bitcoin-denominated fee income for issuing notes
  against your own over-collateralised BTC — long bitcoin, with risk
  bounded by code you can read.
- **For an AI agent**: the only dollar instrument you can natively
  hold and settle — no bank account, no KYC, machine-verifiable.

## 2. Actors

| Actor | Job to be done | Permission |
|---|---|---|
| **Holder** (human or AI agent) | hold $-denominated value; pay (redeem-to-pay); redeem anytime, unilaterally | none |
| **LP / issuer** | issue notes against own over-collateralised BTC; pre-sign their redemptions; earn fees; bear BTC risk | over-collateralised `Q` per rail manifest |
| **Rail operator** (often = LP) | run a rail implementation conforming to spec 02 | none (capacity-bounded, ADR-0002) |
| **Oracle** | attest prices per oracle class (spec 03) | per class; market-priced |
| **Broadcaster / Challenger** | complete settlements; submit dispute evidence | none; paid by protocol economics |
| **List publisher** | publish signed rail-recommendation lists | none; reputation-staked |
| **Founder** | scaffolding roles only (§8); exits | enumerated, with removal criteria |

## 3. User journeys (golden paths)

- **J1 — Acquire**: buy a redemption-bearing note from an LP
  (BTC→note, via an issuance rail; the LP locks over-collateralised
  `Q` and pre-signs the note's redemption CETs). Wallet verifies asset
  lineage, supply commitment, and the pre-signed redemption (spec 07
  §3.4) before displaying balance. No P2P / secondary market — notes do
  not circulate.
- **J2 — Hold & verify**: wallet (or any client) checks supply, the
  note's pre-signed redemption, and settlement history against Bitcoin
  chain data alone. No API of ours needs to exist for J2 to work.
- **J3 — Redeem (unilateral DLC, spec 07)**: the holder broadcasts the
  note's pre-signed CET against the public oracle attestation — one
  Bitcoin tx with the note as a *required input* (burn the note ⟺ claim
  `X/P` BTC). **No LP at redeem-time, no permission, no party can refuse
  or freeze.** Outcome locks at attestation; confirmation on Bitcoin's
  clock. If the LP stops refreshing CETs the holder redeems against the
  last one held (a slightly stale price), never stuck.
- **J4 — LP lifecycle**: lock over-collateralised `Q` (open a vault,
  spec 06) → quote + issue notes → pre-sign each note's redemption +
  maturity CETs → roll fresh CETs per block → earn fees, bear BTC price
  risk (long bitcoin).
- **J5 — Challenge**: detect a deviation (oracle equivocation; in the
  covenant-era reserve, over-cap reimbursement) → submit evidence per
  the dispute hook → collect slash reward. The redemption path itself
  needs no challenge (unilateral, no refusal surface — spec 07 §3.3).
- **J6 — Pay (redeem-to-pay)**: the wallet redeems a note to BTC and
  pays BTC; the counterparty receives BTC while the payer experiences
  spending SatUSD. No SatUSD transfer occurs. The BTC leg MAY ride
  Lightning (spec 08); the SatUSD note stays on L1.

## 4. Functional requirements

Each FR names its normative home and its acceptance check.

| ID | Requirement | Spec | Acceptance |
|---|---|---|---|
| FR-1 | TA grouped asset with supply commitments (tapd ≥ 0.7) and the one-tx burn⟺claim redemption (spec 07 §3) | 01, 07 | mint/burn round-trip on regtest; supply commitment verifiable by independent client |
| FR-2 | Rail standard: state machine, RailManifest, self-certifying rail_id, S1–S3+L1 properties | 02 | conformance test suite; manifest hash vectors Rust = TS |
| FR-3 | **Rail-0 reference implementation** — RFQ atomic-swap **issuance** (BTC→note) | 02 §7 | J1 issuance end-to-end on regtest, then signet |
| FR-4 | **Rail-1 reference implementation** — the oracle-gated DLC **redemption** primitive (TA-in-funding-output) | 02 §7, 07, proposal 0001 | J3 redeem end-to-end on regtest, then signet |
| FR-5 | Oracle daemon: dlcspecs-format announcements/attestations, digit decomposition, BIP-32-derived nonces; explicitly transitional | 03 | independent DLC client can consume its attestations |
| FR-6 | Reserve + epoch allotment **(deferred to the covenant era — ADR-0005; v0 backs each note with its own over-collateralised `Q`)** | 04 | — (returns with the covenant-era shared pool) |
| FR-7 | Client verification library: supply, reserve, lineage, settlement history (S3 artifacts) — embeddable by any wallet | 01/02 | verifies a full settlement history with no access to any server of ours |
| FR-8 | Curation list format: signed, Token-Lists-style; wallet default-subscription semantics | 02 §6.4 | two independent lists, one wallet consuming both |
| FR-9 | Rail disclosure: machine-readable history, age, volume, retained-fee total, stake, dispute record | 02 §6.4 | capacity formula computable by a third party from disclosures + chain |
| FR-10 | Dispute hooks v0: evidence formats + slash flow (Stage 1: published evidence + manual slash per scaffolding ledger; Stage 2: optimistic) | 05 | a planted deviation is detected and slashed end-to-end on signet |
| FR-11 | **Redemption-bearing note**: a pre-signed unilateral DLC redemption bound to each note (spec 07 §3); `committed_term` maturity with fair-value auto-settle | 07, 06, 02 | a note redeems unilaterally on regtest with no LP online; a matured note auto-settles to BTC |

## 5. Non-functional requirements

- **NFR-1 Safety**: every rail demonstrates S1 (unilateral exit),
  S2 (conservation), S3 (observer verifiability), L1 (bought
  liveness) — spec 02 §4 — under test, including adversarial cases.
- **NFR-2 Verifiability**: no protocol claim depends on a server we
  run. Kill all our infrastructure; holders can still verify and
  exit (via timelocks) with nothing but a Bitcoin node.
- **NFR-3 Cross-language correctness**: all consensus-relevant
  encodings pinned by Rust = TS vectors; `make verify` green is a
  merge gate. (Machine-verifiable correctness is the answer to
  single-maintainer review bandwidth.)
- **NFR-4 No permission layer** on issuance/redemption paths — no
  KYC, no freeze, no admin key (MISSION).
- **NFR-5 No-profitable-rug invariant** holds at all times
  (ADR-0002): per-epoch extractable value < verifiable sunk cost.
- **NFR-6 UX targets** (product targets, not protocol guarantees):
  Rail-0 quote→broadcast ≤ 5 s; Rail-1 attestation→broadcast ≤ 1 s
  (outcome lock-in; on-chain confirmation runs on Bitcoin's clock).
- **NFR-7 Cost posture**: service-fee share covers steady-state ops
  at the ADR-0002 thresholds; until then founder-funded, listed in
  the scaffolding ledger.

## 6. Out of scope (v1)

Per MISSION ("What SatUSD is not") and staging decisions:

- yield on holdings; fiat reserves; KYC/compliance features;
  institutional custody integrations
- Lightning carrying SatUSD (LN rides the **BTC leg** of
  redeem-to-pay only — spec 08; the SatUSD note stays on L1)
- `internal_twap` rail (interface reserved in spec 02/03; activates
  only after internal history earns authority)
- the common reserve + reimbursement + capacity formula (deferred to
  the covenant era — ADR-0005; v0 backs each note with its own
  over-collateralised `Q`)
- governance token; legal entity (permanent non-goals, not
  deferrals)

## 7. Milestones — metric-gated, no dates

Per MISSION: phases are recognized by metrics, not calendars. Each
milestone is a checklist; done is done when the checks pass.

**M-A — It runs (regtest).** ✅ 2026-06-10, tag `M-A-regtest`
☑ FR-1..FR-7 implemented · ☑ atomic-swap + DLC-settle E2E green on regtest (Rail-0 swap, Rail-1 DLC) ·
☑ `make check` + cross-language vectors green ·
☑ specs 00/01/03 drafted to implementable precision

**M-B — It's public (signet).**
☐ J1 acquire + J3 redeem completed by ≥ 10 external testers · ☑ oracle daemon
publicly consumable (live: 207.148.98.132:9590, 3-venue median) ·
☐ ≥ 1 external LP quoting · ☑ FR-10 slash drill executed
(2026-06-12, [signet/SLASH_DRILL_FR10.md](../signet/SLASH_DRILL_FR10.md)) ·
☐ informal external review received
*Engineering preconditions complete 2026-06-12: signet genesis
(registry.md), founder LP daemon, holder CLI, quickstart, agent
SDK, repo public; first real redemption confirmed (809fb8a3…).*

**M-C — Real value (mainnet, founder-funded reserve).**
☐ J1–J6 on mainnet · ☐ scaffolding ledger published with the asset ·
☐ security review of consensus-relevant code (form per Q5,
still open)

**M-D — Not just us.**
☐ ≥ 5 independent LPs · ☐ ≥ 2 independent list publishers ·
☐ ≥ 1 third-party rail launched permissionlessly ·
☐ service fees ≥ steady-state ops costs

**M-E — Stage 2 enforcement.**
☐ pre-signed epoch ceremony operational · ☐ manual-allotment
scaffold removed from the ledger

## 8. Scaffolding ledger

The MISSION's closing clause requires transitional compromises to be
enumerated with removal criteria. This table is that enumeration;
it ships with every release.

| Scaffold | Why it exists | Removal criterion |
|---|---|---|
| Founder-funded reserve **(deferred — ADR-0005)** | the common reserve is covenant-era; v0 backs each note with its own per-note `Q` | n/a in v0; the reserve and its removal criterion (vault-minted ≥ 50% of **held** SatUSD, ADR-0004) return with the covenant-era shared pool |
| Founder-run epoch allotment (Stage 1) | enforcement before ceremony tooling exists | M-E: Stage-2 ceremony operational |
| Founder-run single oracle (Rail-1) | dlcspecs oracle market is empty today | ≥ 1 independent oracle class live with market share |
| TA group key custody (issuance authority) | grouped-asset issuance requires a signature; threshold/covenant issuance not yet built (spec 01 §3) | FROST k-of-n group key among independent parties, or covenant-gated issuance |
| Reference marker provider | an external BTC/USD marker is needed until the internal market earns authority (spec 03 §5) | internal_twap switchover criterion met (03 §5.3); external marker demoted to sanity anchor |
| Founder-run universe/proof + disclosure servers | someone must seed S3 availability | ≥ 2 independent mirrors serving the same data |
| Founder as sole maintainer | solo + AI build phase | ≥ 2 maintainers with full release capability |
| Manual slash execution (FR-10 v0) | dispute automation staged | Stage-2 optimistic dispute flow live |

## 9. Top risks

1. **LP cold start** — no liquidity, no product. Mitigation:
   founder LPs the issuance rail at launch (one more ledger entry if
   used); fee economics designed LP-first.
2. **Thin-market manipulation of early S3 history** poisons the
   future internal price source. Mitigation: external reference
   marker as sanity anchor until volume earns authority (MISSION).
3. ~~Reimbursement-pricing model unresolved~~ — resolved 2026-06-11:
   rail settled price within `price_dev_bound` (spec 04 §4); CR
   tiers + NAV floor normative (spec 04 §5). Residual risk: tier
   parameters are conservative guesses until data tunes them.
4. **tapd dependency** — TA-in-keypath-output recognition needs our
   own verifier path (proposal 0001 strategy 1) until upstreamed.
5. **Founder concentration** — bus factor and legal exposure
   (identity decision consciously accepted at first push). Partial
   mitigation is the ledger itself: every founder role has a
   written exit.

## 10. Open product questions

1. Wallet strategy: ship a minimal reference wallet (CLI + thin
   web) or integrate an existing TA wallet first?
2. Quote-board MVP transport (protocol-unstandardized by ADR-0002;
   the *product* still needs one choice — leading candidate:
   Nostr).
3. First target community for the signet alpha (Q5 audit form
   decision rides on the same conversation).
