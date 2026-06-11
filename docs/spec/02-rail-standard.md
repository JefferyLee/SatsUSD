# SatUSD Rail Standard

- **Spec**: 02
- **Version**: 0.2-draft (settle semantics per ADR-0003)
- **Status**: under construction — normative language is aspirational
  until pinned by test vectors; §3/§7 are exercised by the Rail-0
  devnet E2E
- **Authority**: derives from `docs/MISSION.md` (v2) via
  `docs/decisions/ADR-0001-restart-from-mission-v2.md`;
  settle/burn split per ADR-0003
- **Mission criterion**: every design choice below is answerable to
  one question — *does it move us closer to, or further from, the
  state where the external dependency can be removed?*

The key words MUST, MUST NOT, SHOULD, MAY are to be interpreted as
in RFC 2119.

## 1. Concept

A **rail** is a complete SatUSD ↔ BTC conversion channel with its
own service terms: pricing source, settlement primitive, speed,
size limits, fees, LP stake rules, and dispute path. Rails are the
unit of competition: anyone may launch one; wallets verify
conformance cryptographically; users choose; market share is the
judgment.

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
| **User** | Holder converting SatUSD→BTC (redeem) or BTC→SatUSD (mint) | permissionless |
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
    direction:         enum       — redeem | mint | both
    asset_id:          32-bytes   — the TA asset this rail serves
    oracle_spec:       OracleSpec — none | dlcspecs(k, n, [pubkey]) |
                                    optimistic(window) |
                                    internal_twap(window, source_set)
    settle_primitive:  enum       — atomic_swap | dlc_taproot |
                                    optimistic_claim | (extensible)
    max_size:          u64 sats   — per-conversion cap
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

A conforming SETTLE transaction MUST, atomically:
1. move the quoted SatUSD amount out of the user's control, and
2. pay the user the quoted BTC amount, and
3. return any residual to the LP.

The TA leg's destination is the LP's choice (ADR-0003):

- **settle-to-LP** — the LP's own script key. A pure P2P trade;
  the LP may recirculate the SatUSD (market making). No reserve
  interaction occurs and no capacity is consumed.
- **settle-to-burn** — the tapd-native burn key (spec 01 §4) in
  the settle transaction itself; the LP claims reimbursement
  afterwards.

**The burn obligation attaches to reimbursement, not to settle**:
the reserve reimburses only against a burn artifact (spec 04 §1).
The user's guarantee is identical in both modes — quoted BTC
against surrendered SatUSD, atomically.

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

The same machine runs in reverse for `direction = mint` (user
brings BTC, receives newly issued or re-circulated SatUSD). Mint
rails for the senior asset are additionally constrained by the
reserve and CR rules of spec 04. This document defines the channel
mechanics only.

## 4. Required properties

These four properties define conformance. Each MUST hold and MUST
be demonstrable from public data.

**S1 — Unilateral exit.** From every reachable state after quote
acceptance, each party can recover assets worth at least their
contribution minus declared fees, through transitions executable by
themselves *or by any third party*, within a bound fixed by
`settle_window + refund_delta`. No reachable state may require a
specific counterparty's cooperation to avoid loss.

**S2 — Conservation.** Across every terminal state: the SatUSD
moved out of the user's control equals the quoted conversion
amount; no path increases SatUSD supply; supply decreases exactly
by burns, and every reserve reimbursement is backed one-for-one by
a burn artifact (ADR-0003, spec 04); BTC paid to the user equals
the quoted amount at the settled price. (Mint direction: SatUSD
issued equals the quoted amount against BTC received by the
reserve, within spec-04 constraints.)

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

### Rail-0 — RFQ atomic swap (`oracle_spec = none`)

The degenerate base case: price truth is the two signatures.
Reference implementation: `crates/satusd-rail0` (J3 devnet E2E:
`tests/devnet_swap.rs`).

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

### Rail-1 — single-oracle DLC (`oracle_spec = dlcspecs(1,1,[pk])`)

The first oracle-bearing rail; explicitly transitional. Reference
implementation: `crates/satusd-rail1` (+ `satusd-oracle`); the J4
settle E2E (`tests/devnet_settle.rs`) exercises the full chain on a
live devnet, and `tests/devnet_funding.rs` live-validates the
funding construction.

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
