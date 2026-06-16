# SatUSD Rail Standard

- **Spec**: 02
- **Version**: 0.3-draft (note-centric restructure per ADR-0005;
  supersedes the ADR-0003 settle-to-LP model)
- **Status**: under construction — normative language is aspirational
  until pinned by test vectors; §3/§7 are exercised by the Rail-0
  devnet E2E. The §2 manifest changes (`committed_term`; `direction`
  reframed) imply a `satusd-rail` + TS-vector update (follow-up).
- **Authority**: derives from `docs/MISSION.md` (v2) via
  `docs/decisions/ADR-0001-restart-from-mission-v2.md`; note-centric
  model per ADR-0005 (which supersedes ADR-0003's settle/burn split)
- **Mission criterion**: every design choice below is answerable to
  one question — *does it move us closer to, or further from, the
  state where the external dependency can be removed?*

The key words MUST, MUST NOT, SHOULD, MAY are to be interpreted as
in RFC 2119.

## 1. Concept

A **rail** is a competing channel by which an LP **issues** a
redemption-bearing note: the buyer pays BTC and receives a freshly-
minted SatUSD note whose unilateral redemption is pre-signed at
issuance (spec 07). A rail is **not** how redemption happens —
**redemption is the note's own unilateral, oracle-gated DLC, broadcast
by the holder alone (spec 07 §3), with no rail and no LP at
redeem-time.** A rail owns the *issuance* terms — pricing source,
settlement primitive, size limits, fees, committed term — and is the
unit of competition: anyone may launch one; wallets verify conformance
cryptographically; buyers choose; market share is the judgment.

A rail also frames the **BTC leg** of a redeem-to-pay payment (the
holder redeems to BTC and pays BTC; spec 08), but never carries the
SatUSD note itself — notes do not circulate (ADR-0005, spec 07 §8).

The standard exists so that:

1. **Trust is priced by a market, not chosen by a decree** —
   different oracle designs coexist as rails and compete
   (MISSION, principle 2).
2. The **self-referencing rail** (canonical price derived from
   SatUSD's own trade history) wins by offering the best terms,
   not by a governance switch.
3. Every rail's behaviour is **verifiable by anyone** from chain
   data plus the rail's published manifest (MISSION, principle 1).

### 1.1 Roles

| Role | Definition | Permission |
|---|---|---|
| **User** | Holder buying a note (BTC→note, via a rail) or redeeming one (note→BTC, **unilaterally** — spec 07, not via a rail) | permissionless |
| **LP** | Liquidity provider funding the BTC side of a rail | permissionless, may require stake per manifest |
| **Oracle** | Price attestor, per the rail's `oracle_spec`; MAY be absent (Rail-0) | per oracle class (spec 03) |
| **Broadcaster** | Anyone who relays a decryptable/pre-signed transaction | permissionless |
| **Challenger** | Anyone who submits dispute evidence per `dispute_hook` | permissionless |

No role may be restricted to a closed set by a conforming rail,
except Oracle, whose trust profile is exactly what the rail's
manifest discloses and the market prices.

## 2. RailManifest

A rail is defined by its manifest. The manifest is immutable:
**changing any field creates a different rail.**

```
RailManifest {
    spec_version:      u16        — this standard's version
    direction:         enum       — mint (= issue: BTC→note, the v0
                                    rail direction) | redeem | both —
                                    redeem/both are the deferred
                                    reserve-era conversion directions;
                                    v0 redemption is unilateral
                                    (spec 07), not a rail
    asset_id:          32-bytes   — the TA asset this rail serves
    oracle_spec:       OracleSpec — none | dlcspecs(k, n, [pubkey]) |
                                    optimistic(window) |
                                    internal_twap(window, source_set)
    settle_primitive:  enum       — atomic_swap | dlc_taproot |
                                    optimistic_claim | (extensible)
    max_size:          u64 sats   — per-conversion cap
    committed_term:    u32 blocks — the LP's committed holding /
                                    DLC-maturity term for notes issued
                                    on this rail (spec 07 §5; maturity
                                    = LP term; 0 = redeem-only, no
                                    fixed maturity)
    fee_schedule:      {retain_bps: u16, service_bps: u16,
                        fixed_sats: u64}
                                  — retain: accrues to the common
                                    reserve (the costly signal that
                                    feeds the §6 capacity formula);
                                    service: paid per-settlement to
                                    named service providers (oracle,
                                    mirror, broadcaster)
    price_dev_bound:   u16 bps    — max deviation of the settled /
                                    reimbursement price from the
                                    epoch reference marker; MUST be
                                    > 0 for rails drawing reserve
                                    reimbursement
    quote_validity:    u32 sec    — T_quote
    settle_window:     u32 sec    — T_settle
    refund_delta:      u32        — CSV/CLTV blocks after T_settle
    lp_stake:          {amount_sats: u64, slash_conditions: [hash]}
    dispute_hook:      hash       — commitment to the dispute program
                                    (spec 05); 0 = none
    liveness_escape:   enum       — none | tlock(beacon, delay) |
                                    decay_script(params)
    automation_tiers:  map        — per-transition tier declaration
                                    (see §5)
}

rail_id = H(canonical_encode(RailManifest))
```

- `rail_id` is **self-certifying**: no registry issues it, no party
  can squat another's identity, and reputation accrues per-id.
  Human-readable numbers ("Rail-0", "Rail-1") are registry
  conveniences only and carry no rank.
- Canonical encoding follows the workspace canonical-encoding
  rules (`satusd-types`); the exact byte layout MUST be pinned by
  cross-language test vectors before any rail handles mainnet
  value.
- A conforming wallet MUST verify a rail's observed on-chain
  behaviour against its manifest and MUST refuse to render a rail
  whose behaviour deviates from it.
- `lp_stake`, `dispute_hook`, and the §6 capacity formula belong to
  the **deferred** reserve era (ADR-0005, spec 04 scope note): a v0
  note's first-loss is its issuing LP's own over-collateralised `Q`
  (spec 06/07), not a shared bond. `committed_term` and
  `price_dev_bound` apply in v0.

## 3. The conversion state machine

```
            ┌─────────┐   accept    ┌────────┐  condition  ┌────────┐
   offer →  │  QUOTE  │ ──────────► │  LOCK  │ ──────────► │ SETTLE │
            └─────────┘             └───┬────┘             └────────┘
                                        │ T_settle expiry
                                        ▼
                                   ┌────────┐
                                   │ REFUND │
                                   └────────┘

   DISPUTE: an orthogonal overlay; evidence may be submitted at any
   time per dispute_hook; it slashes stakes but MUST NOT block REFUND.
```

### 3.1 QUOTE

The LP produces a signed quote:

```
Quote {
    rail_id, price (or pricing formula), amount,
    expiry = now + quote_validity,
    lock_template_commitment   — hash of the exact LOCK structure
}
```

The quote is an off-chain message. A quote MUST bind the full LOCK
template so the user can verify, before committing anything, that
every downstream path (settle, refund, dispute) is exactly as the
manifest declares.

For collapsed rails (§3.5) the lock template IS the settle's
economic plan: `lock_template_commitment` commits to the full
output economics — conversion amount, user sats, fees (Rail-0:
tag `SatUSD/rail0-lock-template/v1`) — and the user MUST recompute
it and refuse to co-sign a transaction that deviates from the
committed plan.

### 3.2 LOCK

Both legs commit on-chain: the user's SatUSD (a TA-committed input)
and the LP's BTC enter a structure from which only the manifest's
declared paths can spend. Refund paths MUST be armed at lock time —
a lock whose refund depends on future cooperation is non-conforming.

### 3.3 SETTLE

The rail's settlement condition fires:

- **Rail-0 (atomic_swap)**: both signatures on a single transaction
  — LOCK and SETTLE collapse into one atomic step (§3.5).
- **dlc_taproot**: an oracle attestation decrypts the matching
  adaptor signature; any broadcaster completes settlement
  (see `docs/proposals/0001-ta-in-dlc-funding-output.md`).
- **optimistic_claim**: a challenge window elapses unchallenged.

For an **issuance** rail (`direction = mint`, the v0 case), a
conforming SETTLE transaction MUST, atomically:
1. take the buyer's quoted BTC, and
2. deliver the buyer a freshly-issued SatUSD note of the quoted face,
   carrying its pre-signed unilateral redemption (spec 07 §3) over the
   LP's over-collateralised `Q`, and
3. return any residual to the LP.

There is **no settle-to-LP / recirculation path** (retired with
transferability — ADR-0005, superseding ADR-0003): a note is never
handed peer-to-peer. The only disposition of an *existing* note is
**burn-on-redeem** — the note as a *required input* to the burn key in
the spec 07 `redeem_tx` — which is **not** this state machine but the
holder's own unilateral broadcast (spec 07 §3), needing no rail, no LP,
and no oracle-at-settle beyond the public attestation.

### 3.4 REFUND

If no settlement is broadcast within `settle_window`, refund paths
become spendable after `refund_delta` (consensus-enforced timelock).
Refund returns the SatUSD to the user and the BTC to the LP, minus
declared fixed costs only.

### 3.5 Collapsed phases

A rail MAY collapse adjacent phases when atomicity makes the
intermediate state unreachable. Rail-0 collapses LOCK+SETTLE into a
single co-signed transaction: nothing is ever locked without being
simultaneously settled, so no refund machinery is reachable.

### 3.6 Mint direction

`direction = mint` (= **issue**) is the **primary v0 rail
direction**: the buyer brings BTC and receives a newly-issued SatUSD
note (spec 07 §2) — never re-circulated SatUSD (notes do not
circulate). The over-collateralised backing is the LP's own `Q`
(spec 06 §2); the reserve and CR rules of spec 04 are the deferred
reserve-era layer (ADR-0005). This document defines the issuance
channel mechanics; the note's **redemption is spec 07** (unilateral,
not a rail), so the only conversion direction a v0 rail runs is
issuance.

## 4. Required properties

These four properties define conformance. Each MUST hold and MUST
be demonstrable from public data.

**S1 — Unilateral exit.** From every reachable state after quote
acceptance, each party can recover assets worth at least their
contribution minus declared fees, through transitions executable by
themselves *or by any third party*, within a bound fixed by
`settle_window + refund_delta`. No reachable state may require a
specific counterparty's cooperation to avoid loss. For an issued
note, S1 holds **by construction**: its redemption is pre-signed at
issuance and broadcast unilaterally (spec 07 §3.3), so no counterparty
exists to withhold cooperation.

**S2 — Conservation.** Across every terminal state: **issuance**
mints exactly the quoted face against the LP's over-collateralised
`Q` (no path increases supply otherwise); **redemption** decreases
supply by exactly the note burned as a required input (spec 07 §3);
and BTC paid equals the quoted amount at the settled / attested
price. Reserve reimbursement is deferred (ADR-0005); in v0 every
supply change is a direct mint-against-`Q` or a burn-on-redeem, both
observer-verifiable (S3).

**S3 — Observer verifiability.** Every terminal state yields an
artifact chain — txids, TA transfer proofs, oracle attestations —
from which any observer, trusting only Bitcoin consensus, can
verify S2 and reconstruct the settled price. These artifacts are
the raw material of the self-referencing price source: **every
settlement is an implicit price attestation.**

**L1 — Bought liveness.** Every transition that protects a party's
funds MUST be executable by anyone (pre-signed or
attestation-decryptable transactions, CPFP-bumpable by any party),
or fall through to a consensus-enforced timeout. The manifest MUST
declare, per transition, its automation tier (§5). Rails MUST NOT
depend on RBF for fee escalation of pre-signed transactions; an
anchor output spendable by either party MUST be present where fees
may need bumping.

## 5. Automation tiers

Each transition in `automation_tiers` declares where its liveness
comes from:

| Tier | Source | Example |
|---|---|---|
| **1 — consensus** | validity flips at a timelock; no actor needed beyond a broadcast that anyone can perform | REFUND paths |
| **2 — anyone-can-act** | pre-signed / decryptable artifact + economic incentive | CET broadcast, challenge submission |
| **3 — federation-crypto** | a public beacon's routine operation releases a secret | `tlock` liveness escape: oracle silence auto-degrades to refund authorization after T+δ |

Tier 2 is the default posture (MISSION, principle 3: liveness is
bought, not assumed). Tier 3 escapes are RECOMMENDED for rails
whose oracle can stall settlement. A rail relying on any *specific*
party's future action for fund safety is non-conforming by S1.

## 6. Capacity, not identity

The standard does not ask "is this rail trustworthy?" (an identity
question requiring an adjudicator). It asks "how much harm can
this rail cause?" (a quantity question answerable by formula).
There is no admission, no graduation event, and no gatekeeper —
only a mathematically bounded growth curve that any observer can
recompute from public data.

> **v0 scope (ADR-0005).** The capacity *formula* below (§6.1–§6.3)
> caps a rail's draw on the **common reserve**, which is **deferred to
> the covenant era** (spec 04 scope note). In v0 there is no shared
> reserve to draw on: a note's harm bound is its issuing LP's **own
> over-collateralised `Q`** (spec 06/07) — a per-note first-loss, not
> a pooled cap. The §6.1 formula, `lp_stake`, and the Layer-4 reserve
> return with the covenant-era shared pool. The advisory layer (§6.4
> curation lists) and naming discipline (§6.5) apply in v0 unchanged.

### 6.1 The capacity formula

Per epoch (spec 04 defines the epoch cadence), each rail's draw on
the common reserve is capped:

```
capacity(rail, epoch) = α × (retained_fees_cum + lp_stake)
```

- `retained_fees_cum` — the rail's cumulative `retain_bps` fees,
  verifiably accrued to the common reserve (computable from S3
  artifacts + reimbursement history). Service fees do NOT count:
  they are paid to potentially-colluding parties and can be faked
  by self-dealing; retained fees are donations to all holders and
  cannot be.
- `lp_stake` — the rail's posted first-loss bond, denominated in
  BTC only (SatUSD stake would be reflexive and is non-conforming).
- `α` — a spec-level constant (initially **0.5**), NOT a manifest
  field. Rails cannot choose their own safety factor.

### 6.2 The no-profitable-rug invariant

With α < 1, the maximum a rail can extract through misbehavior in
one epoch is strictly less than its verifiable sunk cost
(forfeited stake + already-donated fees). Building a position
worth attacking costs more than the attack can ever return.
Track record becomes a costly signal that cannot be faked: wash
volume requires paying real retained fees, which accrue to all
holders at 1:1 cost to the faker.

There is deliberately **no time factor** in the protocol formula.
The invariant holds for a day-old whale and a year-old minnow
alike; age-based caution belongs to the advisory layer (§6.4),
where it can be applied without slowing honest capital.

### 6.3 Defense in depth

```
Layer 1   price_dev_bound      per-settlement price deviation cap
Layer 2   capacity(epoch)      aggregate exposure cap (§6.1)
Layer 3   lp_stake             first-loss: shortfalls hit the
                               rail's own bond before anything else
Layer 4   common reserve       touched only if 1–3 are exhausted
```

Enforcement of Layer 2 is the reserve's epoch allotment mechanism
(spec 04): per-rail tranches sized by the formula, with a
three-stage hardening path — transparent manual allotment →
pre-signed epoch-boundary ceremony → covenant/BitVM enforcement.

### 6.4 The advisory layer: curation lists

Capacity bounds harm; it does not recommend. Discovery and
recommendation are a separate, competitive layer modeled on the
Token Lists precedent: **anyone** may publish a signed rail list
with their own criteria; wallets ship default subscriptions; users
may switch or add lists. List publishers compete on reputation —
recommending a rail that later misbehaves is publicly costly.
Judgment is not eliminated; it is marketized, exactly as
oracle choice is.

To make those judgments possible, disclosure is normative:

- a rail MUST expose machine-readable pointers to its full
  settlement history (S3 artifacts), and
- a rail MUST declare its age, cumulative volume, retained-fee
  total, stake, and dispute record in verifiable form.

RECOMMENDED list heuristics (non-normative): ≥ 90 days of
operation, zero unresolved disputes, settled-price coherence with
the reference marker within `price_dev_bound`.

### 6.5 Experimental assets

Separate TA assets ("SatUSD-X[n]", separate reserves) are NOT
needed for trialing new rails — capacity bounds make the canonical
asset safe for permissionless rail entry. They remain available
for experiments that change the **asset rules themselves**
(different reserve model, CR formula, burn semantics). Naming
discipline: the unsuffixed name is reserved for the canonical
asset; conforming wallets MUST display experimental assets with
their suffix and asset_id.

## 7. Reference rails

### Rail-0 — RFQ atomic-swap issuance (`oracle_spec = none`)

The degenerate base case and the **v0 issuance rail**: a buyer pays
BTC and receives a freshly-issued note in one co-signed atomic
transaction; price truth is the two signatures. The atomic-swap
construction is symmetric — the reference E2E
(`crates/satusd-rail0`, `tests/devnet_swap.rs`) exercised the swap
leg-roles directly; v0 uses it in the **issuance direction**
(BTC→note), the redeem-direction swap being retired (redemption =
spec 07, unilateral). Validating the issuance direction end-to-end
is a follow-up.

- QUOTE: RFQ board (transport unspecified); LPs respond with signed
  quotes; user picks. `lock_template_commitment` commits to the
  SwapPlan (§3.1).
- LOCK+SETTLE (collapsed): one anchor transaction — inputs: the
  user's TA commitment UTXO + the LP's BTC UTXO; outputs: the TA
  outputs (destination per §3.3 + asset change) + BTC to user +
  LP residual. Built via tapd's external-anchor flow
  (`FundVirtualPsbt → SignVirtualPsbt → CommitVirtualPsbts` with
  the rail's anchor template → `PublishAndLogTransfer`); every
  fact the template needs is parsed from the funded vPSBT itself.
  Co-signed; atomic by construction.
- Non-anchor P2TR outputs (payouts, change) MUST carry their
  taproot internal key in the PSBT — TA exclusion proofs require
  it for every P2TR output of the anchor transaction. Non-P2TR
  payouts avoid the requirement.
- REFUND: unreachable (nothing locks without settling).
- DISPUTE: none needed for the swap itself; LP quote manipulation
  is policed by competition and by S3 history (systematic deviation
  from the reference marker is publicly computable).

Rail-0 is the day-one rail and the seed of the self-referencing
price source: its settlement history is the data from which an
`internal_twap` rail later derives authority.

### Rail-1 — the oracle-gated DLC redemption primitive (`oracle_spec = dlcspecs(1,1,[pk])`)

This is **the `dlc_taproot` construction spec 07 §3 uses for a note's
unilateral redemption**: a DLC funding output carrying the
over-collateralised `Q`, with per-bucket adaptor CETs an oracle
attestation decrypts. It is not a separate rail a holder "chooses" at
redeem-time — it is how every note redeems (armed at issuance,
broadcast unilaterally). Explicitly transitional: single oracle →
FROST aggregate (spec 03 §5.7). Reference implementation:
`crates/satusd-rail1` (+ `satusd-oracle`); the settle E2E
(`tests/devnet_settle.rs`) exercises the full chain on a live devnet,
and `tests/devnet_funding.rs` live-validates the funding
construction.

- QUOTE: as Rail-0, with price expressed as a CET bucket schedule —
  aligned binary digit-prefix wildcards, the 2^m special case of
  dlcspecs NumericOutcomeCompression (one adaptor presign per
  bucket, not per outcome).
- LOCK: the DLC funding output carrying the TA commitment leaf and
  a CSV refund leaf — construction per
  `docs/proposals/0001-ta-in-dlc-funding-output.md`; tapd supports
  it natively (NewAddr `tapscript_sibling` + internal-key override).
  v0 uses a single funding key; MuSig2 aggregation is the declared
  upgrade (the secp256k1 crate currently lacks a musig module).
- SETTLE: the oracle attestation's per-digit scalars sum to the
  winning bucket's secret, which decrypts that bucket's adaptor
  pre-signature into a BIP-341 keyspend of the funding output; any
  broadcaster completes. CETs are made fully deterministic at LOCK
  time via `CommitVirtualPsbts` with `skip_funding`.
- REFUND: CSV script path after `settle_window + refund_delta`.
- Liveness escape (RECOMMENDED): `tlock(drand, T+δ)` on the
  oracle's nonce secret — oracle silence degrades to refund
  authorization without waiting out the full refund delta.
- DISPUTE: oracle equivocation is self-punishing (per-event
  committed nonces make double-signing key-extractable); stake
  slash per spec 05.

## 8. Security considerations

- **Adverse selection / look-back optionality** re-emerges per rail
  in volatile moments and is *deliberately not solved at the
  standard level*: each rail prices it via `fee_schedule`,
  `price_dev_bound`, and `settle_window`. The market clears it.
- **Thin-market manipulation** of the S3-derived price history is
  the principal early risk to the self-referencing path; external
  markers remain the sanity anchor until internal volume earns
  authority (MISSION: anchors demote to sanity checks, they do not
  disappear by decree).
- **Oracle silence** is a liveness attack, not a safety attack,
  under S1 — funds are never lost, only delayed; Tier-3 escapes
  bound the delay.
- **Fee spikes** at settlement time are handled by anchor outputs +
  CPFP (L1); rails MUST budget `refund_delta` to survive sustained
  congestion.
- **Name hijacking** is mitigated by self-certifying rail_ids and
  the §6 naming discipline; reputation cannot be transferred or
  forged, only accrued.

## 9. Open questions

1. ~~Exact canonical byte encoding of RailManifest~~ — resolved:
   Rust-side vectors pinned (`satusd-rail::manifest` tests) and the
   TS mirror verifies them byte-for-byte (`ts/src/rail.ts`,
   `integration/vectors/rail-vectors.json`, `make verify`).
2. Whether `internal_twap` oracle_spec needs its own spec section
   (likely: spec 03 §internal) covering window, volume weighting,
   and outlier rules — the S3 artifact format must anticipate it.
3. Capacity amplification: a refined formula
   `capacity = α × (retained + stake) / price_dev_bound` is more
   capital-efficient (tighter dev bounds earn more capacity) but
   depends on pinning the reimbursement-pricing model in spec 04.
   v0.1 keeps the conservative un-amplified form; revisit after
   spec 04 settles.
4. The reference marker used for `price_dev_bound` and
   reimbursement pricing — its source set and update cadence —
   is owned by spec 03/04 and is the principal remaining external
   dependency on the self-referencing path.

Resolved this revision: quote transport is NOT standardized (only
the Quote payload is; transport is rail-implementation choice);
no on-chain rail allowlist (capacity formula replaces admission —
see ADR-0002); LP stake is BTC-only (reflexivity).
