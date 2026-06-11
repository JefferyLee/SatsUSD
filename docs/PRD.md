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
on Bitcoin L1, convertible to and from BTC at any time through an
open standard of competing conversion rails. Holding it requires no
permission; redeeming it requires no trusted intermediary; verifying
it requires only Bitcoin.

One sentence per audience:

- **For a person under capital controls or debanking**: dollars that
  no company and no government can freeze, in your own keys.
- **For a bitcoiner**: spendable dollar stability without leaving
  Bitcoin L1 or touching a fiat reserve.
- **For an LP**: bitcoin-denominated fee income for providing
  conversion liquidity, with risk bounded by code you can read.
- **For an AI agent**: the only dollar instrument you can natively
  hold and settle — no bank account, no KYC, machine-verifiable.

## 2. Actors

| Actor | Job to be done | Permission |
|---|---|---|
| **Holder** (human or AI agent) | hold $-denominated value; pay; redeem anytime | none |
| **LP** | quote conversions, front BTC, earn fees | stake per rail manifest |
| **Rail operator** (often = LP) | run a rail implementation conforming to spec 02 | none (capacity-bounded, ADR-0002) |
| **Oracle** | attest prices per oracle class (spec 03) | per class; market-priced |
| **Broadcaster / Challenger** | complete settlements; submit dispute evidence | none; paid by protocol economics |
| **List publisher** | publish signed rail-recommendation lists | none; reputation-staked |
| **Founder** | scaffolding roles only (§8); exits | enumerated, with removal criteria |

## 3. User journeys (golden paths)

- **J1 — Acquire**: mint via a mint-direction rail, or buy P2P /
  on-market. Wallet verifies asset lineage + supply commitment
  before displaying balance.
- **J2 — Hold & verify**: wallet (or any client) checks reserve,
  supply, and settlement history against Bitcoin chain data alone.
  No API of ours needs to exist for J2 to work.
- **J3 — Redeem via Rail-0 (RFQ)**: request quote → LPs respond →
  user picks → one co-signed atomic transaction: the SatUSD leaves
  the user's keys (to the LP, or straight to the burn key —
  ADR-0003), BTC arrives. No oracle anywhere.
- **J4 — Redeem via Rail-1 (DLC)**: quote (CET schedule) → lock →
  oracle attests at tick → anyone broadcasts the matching CET →
  outcome locked at attestation, confirmation on Bitcoin's clock.
- **J5 — LP lifecycle**: post stake → publish quotes → settle →
  claim reimbursement from epoch tranche (burn proof, minus
  retain haircut) → fee income.
- **J6 — Challenge**: detect a deviation (over-cap reimbursement,
  bad proof, oracle equivocation) → submit evidence per dispute
  hook → collect slash reward.
- **J7 — Pay**: transfer SatUSD as a TA on L1. (Lightning rails:
  out of scope v1, see §6.)

## 4. Functional requirements

Each FR names its normative home and its acceptance check.

| ID | Requirement | Spec | Acceptance |
|---|---|---|---|
| FR-1 | TA grouped asset with supply commitments (tapd ≥ 0.7) and burn-to-NUMS redemption semantics | 01 | mint/burn round-trip on regtest; supply commitment verifiable by independent client |
| FR-2 | Rail standard: state machine, RailManifest, self-certifying rail_id, S1–S3+L1 properties | 02 | conformance test suite; manifest hash vectors Rust = TS |
| FR-3 | **Rail-0 reference implementation** — RFQ atomic swap | 02 §7 | J3 end-to-end on regtest, then signet |
| FR-4 | **Rail-1 reference implementation** — single-oracle DLC with TA-in-funding-output | 02 §7, proposal 0001 | J4 end-to-end on regtest, then signet |
| FR-5 | Oracle daemon: dlcspecs-format announcements/attestations, digit decomposition, BIP-32-derived nonces; explicitly transitional | 03 | independent DLC client can consume its attestations |
| FR-6 | Reserve + epoch allotment Stage 1: open-source allotment script, published plans, reproducible by anyone | 04 | third party re-runs script on public data and reproduces the plan byte-for-byte |
| FR-7 | Client verification library: supply, reserve, lineage, settlement history (S3 artifacts) — embeddable by any wallet | 01/02 | verifies a full settlement history with no access to any server of ours |
| FR-8 | Curation list format: signed, Token-Lists-style; wallet default-subscription semantics | 02 §6.4 | two independent lists, one wallet consuming both |
| FR-9 | Rail disclosure: machine-readable history, age, volume, retained-fee total, stake, dispute record | 02 §6.4 | capacity formula computable by a third party from disclosures + chain |
| FR-10 | Dispute hooks v0: evidence formats + slash flow (Stage 1: published evidence + manual slash per scaffolding ledger; Stage 2: optimistic) | 05 | a planted deviation is detected and slashed end-to-end on signet |

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
- Lightning rails (L1 settlement first; LN is a later rail class)
- `internal_twap` rail (interface reserved in spec 02/03; activates
  only after internal history earns authority)
- Stage 2/3 reserve enforcement (staged per ADR-0002; v1 ships
  Stage 1)
- governance token; legal entity (permanent non-goals, not
  deferrals)

## 7. Milestones — metric-gated, no dates

Per MISSION: phases are recognized by metrics, not calendars. Each
milestone is a checklist; done is done when the checks pass.

**M-A — It runs (regtest).** ✅ 2026-06-10, tag `M-A-regtest`
☑ FR-1..FR-7 implemented · ☑ J3 + J4 E2E green on regtest ·
☑ `make check` + cross-language vectors green ·
☑ specs 00/01/03 drafted to implementable precision

**M-B — It's public (signet).**
☐ J3/J4 completed by ≥ 10 external testers · ☐ oracle daemon
publicly consumable · ☐ ≥ 1 external LP quoting · ☐ FR-10 slash
drill executed · ☐ informal external review received

**M-C — Real value (mainnet, founder-funded reserve).**
☐ capacity formula live with conservative caps · ☐ J1–J5 on
mainnet · ☐ scaffolding ledger published with the asset ·
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
| Founder-funded reserve | bootstrap without VC or entity | external LP/holder base large enough that founder share < 50% of reserve |
| Founder-run epoch allotment (Stage 1) | enforcement before ceremony tooling exists | M-E: Stage-2 ceremony operational |
| Founder-run single oracle (Rail-1) | dlcspecs oracle market is empty today | ≥ 1 independent oracle class live with market share |
| TA group key custody (issuance authority) | grouped-asset issuance requires a signature; threshold/covenant issuance not yet built (spec 01 §3) | FROST k-of-n group key among independent parties, or covenant-gated issuance |
| Reference marker provider | an external BTC/USD marker is needed until the internal market earns authority (spec 03 §5) | internal_twap switchover criterion met (03 §5.3); external marker demoted to sanity anchor |
| Founder-run universe/proof + disclosure servers | someone must seed S3 availability | ≥ 2 independent mirrors serving the same data |
| Founder as sole maintainer | solo + AI build phase | ≥ 2 maintainers with full release capability |
| Manual slash execution (FR-10 v0) | dispute automation staged | Stage-2 optimistic dispute flow live |

## 9. Top risks

1. **LP cold start** — no liquidity, no product. Mitigation:
   founder LPs Rail-0 at launch (one more ledger entry if used);
   fee economics designed LP-first.
2. **Thin-market manipulation of early S3 history** poisons the
   future internal price source. Mitigation: external reference
   marker as sanity anchor until volume earns authority (MISSION).
3. **Reimbursement-pricing model unresolved** (spec 04 §4) — the
   binding design item; blocks Rail-1 mainnet, not Rail-0 regtest.
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
