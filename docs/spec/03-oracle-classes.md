# SatUSD Oracle Classes & the Reference Marker

- **Spec**: 03
- **Version**: 0.2-draft (adds the adaptive-blend marker evolution, §5.4–§5.5)
- **Authority**: `docs/MISSION.md` v2 via ADR-0001
- **Depends on**: 00 (conventions), 02 (manifest `oracle_spec`,
  capacity / `retained_fees_cum`), dlcspecs (Oracle.md, Messaging.md,
  NumericOutcome*)

Defines the oracle classes a RailManifest may declare, the
attestation formats, equivocation handling, liveness escapes, and
the **reference marker** — the protocol-level price every other spec
leans on. The marker is not a fixed source but a *self-dissolving
scaffold*: it begins by reflecting the external market and evolves —
weighted by **cost-to-manipulate**, never by calendar or decree —
into SatUSD's own self-referencing price (§5.4), while the external
source demotes from a price input to a permanent catastrophe fuse.
The mission criterion governs throughout: each class exists to be
outcompeted by the one below it on the list.


## 0. Oracle posture

The single-signer oracle running today is this system's
self-declared weakest link — a PRD §8 scaffolding row with a
written removal criterion, not a foundation. The protocol's
security does not rest on the oracle's honesty; it rests on the
**boundedness of its dishonesty**:

- equivocation surrenders the oracle's secret key to anyone (§3.3,
  implemented and tested);
- a single false price is bounded by `price_dev_bound` per
  settlement and by capacity per rail (§3.3 scope note);
- silence is bounded by the CSV refund path — users exit
  unilaterally.

Above that floor, oracles are a free-entry market: any rail brings
its own trust model (§1 classes), users select with real money,
and capacity bounds each entrant's blast radius. The market's true
product is not "the best oracle" — it is **volume**. Every
settlement is an implicit price attestation (spec 02 §4 S3); when
the settlement history is thick enough, the price signal stops
needing any signer at all (§5, volume-gated). The competing
oracles are not aggregated by the end state — they are
*metabolized* by it, having served their historical purpose of
bootstrapping the flow.

## 1. Class registry

```
OracleSpec :=
  | none                                  — class 0 (Rail-0)
  | dlcspecs { k: u16, n: u16,
               oracle_pubkeys: [x-only; n],
               event_template: EventTemplate }
  | optimistic { window_blocks: u32,
                 bond_sats: u64 }          — reserved (§6)
  | internal_twap { window_blocks: u32,
                    min_volume_usd: u64 }  — end state (§5.2)
```

`canonical_encode` per 00 §3; the enum discriminant order above is
normative (0..3).

## 2. Class `none`

Price truth is the two settlement signatures (02 §7 Rail-0). No
announcements, no attestations, no oracle liveness assumption. The
settled price still enters S3 history and is still subject to the
rail's `price_dev_bound` against the reference marker **at
reimbursement time** (04 §4) — bilateral freedom does not extend to
the common reserve.

## 3. Class `dlcspecs`

### 3.1 Event template (BTC/USD ticks)

```
EventTemplate {
    pair:        "BTCUSD" (UTF-8)
    cadence_s:   u32   — seconds between events (Rail-1 default: 1)
    base:        u16 = 2
    nb_digits:   u16 = 20         — covers 0..1,048,575 USD/BTC
    is_signed:   false
    precision:   i32 = 0          — whole USD per BTC
    unit:        "USD/BTC"
}
event_id  = "SatUSD/BTCUSD/" ‖ decimal(unix_ts)
maturity  = unix_ts
```

Outcome value: the BTC/USD price in whole dollars, round half up,
sourced per the oracle's declared methodology (announcement
metadata). Conversion to the canonical price type per 00 §3.6.
Overflow: sign the maximal value (all-ones), per dlcspecs.

### 3.2 Announcements & attestations

Wire formats are **dlcspecs-native**: `oracle_announcement` and
`oracle_attestation` TLVs per dlcspecs Messaging.md; signatures
BIP-340 with the dlcspecs tags (`announcement/v0`,
`attestation/v0`). SatUSD adds no custom envelope — any existing
DLC client can consume a conforming SatUSD oracle.

The announcement signature is a regular BIP-340 signature over
`tagged_hash("DLC/oracle/announcement/v0", oracle_event_tlv)`.
Announcements MUST be published at least `2 × cadence_s × 60`
ahead of maturity, in batches (RECOMMENDED: one batch per minute
covering the next two minutes of events at 1 s cadence). Wire
vectors for both message types are pinned in `satusd-oracle`.

Transport is **not** part of this standard — only the TLVs are.
The reference daemon (`oracled`) serves them as hex over HTTP:
`GET /v0/pubkey`, `/v0/announcement/<unix_ts>`,
`/v0/attestation/<unix_ts>`, `/v0/latest`. Any mirror serving the
same bytes is equivalent; clients MUST verify signatures, never
endpoints.

### 3.3 Nonce discipline

Per-event, per-digit nonces MUST be derived deterministically from
a secret seed: `k_i = int(tagged_hash("SatUSD/oracle/nonce/v1",
seed ‖ event_id ‖ u16_be(digit_index)))` — the digit index is part
of the derivation, one nonce per digit per event; **no nonce may
ever sign two different messages**. Seeds MUST NOT be reused across
oracle public keys. Reference implementation:
`satusd-oracle::event::nonce` (the library verifier round-trips
every attestation signature in tests).

This gives EOTS-style accountability for free: two attestations for
the same event with different outcomes expose the nonce equation
system and allow anyone to extract the oracle's secret key.
The pair of conflicting attestations is itself the equivocation
evidence artifact (05); no further proof is needed.

**Scope of the punishment (normative honesty).** Nonce-reuse
extraction punishes *equivocation only*. An oracle that signs a
single false price faces no cryptographic penalty — this is the
known limitation of all DLC oracles. The protocol bounds that
behavior economically rather than cryptographically:

- reimbursement requires the settled price within
  `price_dev_bound_bps` of the reference marker (spec 04 §4) — a
  lie's extractable value is bounded per settlement;
- capacity bounds the lying rail's blast radius (ADR-0002);
- k-of-n composition (§3.4) dilutes a single signer;
- rail marketization prices oracle quality in fees and volume.

*Eliminating* — rather than bounding — this residual trust is
exactly the `internal_twap` end state (§5): no signer, nothing to
lie with.

### 3.4 k-of-n composition

`(k, n)` with `n > 1` composes per dlcspecs MultiOracle.md
(enumerated t-of-t aggregation over the C(n,k) subsets, with
bounded-error parameters `minSupportExp`/`maxErrorExp` declared in
the rail's quote). v0.1 normatively supports `(1,1)` and `(k ≤ 3,
n ≤ 5)`; larger sets await the adaptor-count economics review.

`(1,1)` is **transitional by declaration**: a rail whose oracle_spec
is single-signer MUST say so in its disclosure (02 §6.4), and the
founder-run instance is a PRD §8 ledger row.

### 3.5 Liveness escape (tlock)

A dlcspecs-class oracle SHOULD publish, alongside each announcement
batch, the batch's nonce secrets encrypted to a drand round:

```
TlockEscape {
    drand_chain:   32 bytes  — chain hash (default: quicknet)
    round:         u64       — round( max_maturity + delay_s )
    ciphertext:    varbytes  — tlock(IBE) of concat(k_i) for batch
}
```

If the oracle goes silent, the beacon's routine operation releases
the secrets after `delay_s` (RECOMMENDED 600 s), at which point
**every** outcome becomes signable — by construction the rail's
lock template routes that condition to the refund-equivalent CET
only (02 §3.4, proposal pending for the exact CET wiring). Oracle
silence is thereby a bounded delay, never a fund loss (Tier-3
automation, 02 §5).

## 4. Class `optimistic` (reserved)

Interface reserved for UMA-style assert-challenge resolution of
slow, large conversions. Manifest params (`window_blocks`,
`bond_sats`) are pinned now so manifests hash stably; settlement
artifact format and escalation rules are deferred until a rail
wants them. Not required for Rail-0/1 or any M-A..M-C milestone.

## 5. The reference marker

The single protocol-level BTC/USD price feeding `price_dev_bound`
checks (02) and reimbursement pricing (04 §4).

### 5.1 Transitional implementation: external marker

```
MarkerAttestation {
    height:     u32       — Bitcoin block height it applies from
    price:      u64       — price_msat_per_usd (00 §3.6)
    method:     32 bytes  — hash of the published methodology doc
    signature:  64 bytes  — BIP-340 by the marker key, tag
                            "SatUSD/marker/v1"
}
```

- Cadence: at least one attestation per 6 blocks (~hourly).
- Methodology (published, hash-committed): median of at least three
  independent external sources, outlier-trimmed; the methodology
  doc names them.
- **Scaffolding**: the marker provider is founder-run at launch
  (PRD §8 row). Anyone can run a competing marker; rails name their
  marker key in disclosures; curation lists judge marker choice
  like everything else.

### 5.2 End state: `internal_twap`

The marker the mission requires. Computation, normative now so
`satusd-verify` can implement it ahead of activation:

```
inputs:  S3 settlement artifacts for the asset within window_blocks
         (default 144) ending at height h, each weighted by its
         costly-signal-backed volume (§5.6 A), not raw volume
filter:  drop settlements whose price deviates > 5 % from the raw
         median (the lower-middle element for even counts)
output:  volume-weighted median price of the survivors — the first
         price, ascending, whose cumulative volume reaches
         ceil(total/2); ties resolve to the earlier price
valid:   only if surviving volume ≥ min_volume_usd; otherwise the
         marker falls back to §5.1 for that window
```

Reference implementation: `satusd-verify::marker::internal_twap`,
including the anti-manipulation test (an off-market whale with
100× the honest volume is trimmed and cannot move the marker).

Every input is chain-derived (S3): **any observer computes the same
number**. There is no internal-marker signer — it is a pure
function of public data; rails reference it by declaring
`internal_twap` parameters instead of a marker key.

### 5.3 Switchover

No decree. Clients and rails migrate marker source when, per their
own (or their curation lists') criteria, the internal marker has
earned authority. RECOMMENDED criterion, machine-checkable:
internal_twap valid (volume floor met) for ≥ 90 consecutive
windows AND median |internal − external| < 0.5 % over that span.
The external marker then demotes to the sanity anchor (MISSION:
anchors demote; they do not disappear by decree) — divergence
> 2 % between the two SHOULD trigger client-side caution states,
and spec 04 MAY bind reimbursement pauses to it.

### 5.4 Adaptive blend — the evolution mechanism

§5.1–§5.3 give two marker sources and a binary switch between them.
The mission targets a single price that *evolves continuously* from
external to internal as the internal market earns it; the §5.3
switchover is the conservative degenerate case (a step weight) used
in v0.1 until the estimator below is pinned by adversarial
simulation (§6).

**Why begin external at all.** SatUSD's trades depend on the marker
to be priced; without a trustworthy marker there is no flow, and
without flow there is no internal price to discover. The marker is
therefore a scaffold: a good external-reflecting marker bootstraps
the very market whose settlements (S3) later replace it. *The marker
is built to dissolve itself.*

**Blended marker.**

```
P_marker(h) = w(h)·P_internal(h) + (1 − w(h))·P_external(h)
clamp:        |P_marker(h) − P_external(h)| ≤ band(h)
```

with internal weight `w ∈ [0,1]`. `P_internal` is the §5.2
`internal_twap`; `P_external` is the §5.1 marker (SHOULD be the
decentralised aggregate of §5.5, not a single signer).

**Weight by cost-to-manipulate — never by calendar or decree.**
`w(h)` MUST be a monotone function of the internal market's realised
**cost-to-manipulate (CtM)** relative to the external source's,
estimated only from public, costly-to-fake data:

- The driver is the rail standard's existing un-fakeable signal:
  cumulative retained fees (`retained_fees_cum`, 02 §6.1) and
  realised settlement depth in-window. A wash-trader inflates either
  only by paying real 1:1 costs to all holders; raw volume, free to
  fake, MUST NOT drive `w`.
- **CtM cap (the reflexivity bound).** At no height may the internal
  price carry more weight than the cost to move it can defend:
  `w(h) ≤ CtM_int(h) / (CtM_int(h) + CtM_ext(h))`. A thin internal
  market cannot acquire weight it cannot defend.
- No time term: a high-volume week and a year of trickle reach the
  same `w` iff they bought the same defensible depth.

**External's two roles — a shrinking weight, a permanent fuse.** As
`w → 1` the external *weight* `(1 − w)` vanishes, but the *clamp*
never does:

- `band(h)` MAY widen as internal earns authority (the internal
  price is then allowed to diverge from external, being the
  harder-to-fake number) but MUST stay finite. Its asymptote is a
  **catastrophic-divergence circuit-breaker**: beyond it `P_marker`
  is `invalid`, and consuming specs (04 reimbursement, 06 vault
  liquidation) MUST halt rather than act on a number no source can
  defend.
- External therefore never becomes a *dependency*; it ends as a
  *fuse* that almost never binds. This is how "remove the external
  dependency" is reached without pretending the external world
  vanished.

**Reflexivity is the governing risk.** `P_marker` guides the very
trades that produce `P_internal`; the handoff is a feedback loop. A
thin internal market granted premature weight could be moved
cheaply, dragging the marker, validating the move — reflexive
capture. The CtM cap on `w` and the clamp together damp it: internal
weight rises no faster than internal defensible depth, and the clamp
stops a manipulated thin market from pulling the marker outside the
external band. The damping parameters — how fast `band(h)` widens,
the CtM estimator's window — are safety-critical and MUST be pinned
by adversarial simulation before any non-degenerate `w` is activated
(§6).

**Read the marker by horizon (defence in depth, not a contradiction
of 1 s trading).** A one-tick marker move has small
profit-from-corruption (it mis-prices only that tick's conversion);
a *sustained* move has large PfC (it triggers liquidations, skews
reimbursement). Therefore:

- trade pricing MAY consume the freshest `P_marker` tick;
- liquidation / reimbursement / mint MUST consume a windowed,
  manipulation-resistant form (the `internal_twap` window already is
  one) — these transitions are block-paced anyway, so the window is
  free.

This makes single-tick corruption low-reward and sustained
corruption high-cost, independent of the blend weight.

### 5.5 Decentralising the external source (bootstrap anchor)

While external weight dominates, `P_external` SHOULD be a
decentralised, manipulation-resistant aggregate, not the single
founder-run marker of §5.1. RECOMMENDED form — a free-entry
**stake-weighted median**:

- reporters post a BTC bond; each tick they commit-then-reveal a
  price (commit-reveal prevents copying the emerging median);
- `P_external` = the stake-weighted median of reveals (> 50 % of
  stake required to move it);
- "correct" = within a **volatility-adaptive band** of the median;
  the reward (a `service_bps` fee, 02) is split **stake-
  proportionally** among in-band reporters (flat per-reporter splits
  are Sybil-farmable; stake-weighting is not);
- only **gross outliers** (beyond the band) are slashed — honest
  noise and genuine dislocations are not. Slashing settles with a
  1–3 block deferral, and bond withdrawal is delayed past the
  settlement window so an attacker cannot profit-and-exit.

This aggregate is a **relay, not a discoverer**: every reporter
reads the same external venues, so the median *reflects* the
arbitrage-linked exchange price and structurally **lags** it. That
is expected and acceptable — its product is *sovereignty and
manipulation / censorship resistance* of the bootstrap anchor, not
price leadership, which no relay can have (price discovery happens
only where real capital trades — exactly what the §5.2 internal
settlements become). Security holds while
`total_honest_stake > value extractable within the slash window`,
**including external short positions on SatUSD**: a captor who
profits from SatUSD's collapse defeats the "don't kill the golden
goose" incentive, so the stake floor MUST exceed that external
payoff, not merely the in-protocol one.

### 5.6 Coupling with rail competition (spec 02)

The marker is built from the very settlements rail competition
produces, and in turn bounds them via `price_dev_bound`. The two are
mutually reinforcing — many independent, capacity-bounded rails make
`internal_twap` broad and hard to corner; the marker keeps
reserve-touching settlements honest — **but the loop becomes a
manipulation vector unless it obeys three rules.**

**A — Weight by the costly signal, not raw volume.** A settlement's
weight in `internal_twap` (§5.2) is its conversion size **capped per
rail at that rail's epoch capacity** (`α·(retained_fees_cum +
lp_stake)`, 02 §6.1). Pure `settle-to-LP` volume that paid no
`retain_bps` (02 §3.3) is self-dealable for the cost of a miner fee
and **MUST NOT count** toward the marker; only retained-fee-backed
(reserve-touching, capacity-bounded) volume does. One un-fakeable
signal thus does three jobs: bound rail capacity (02 §6), weight
`internal_twap` (§5.2), and drive the blend weight `w` (§5.4).

**C — A settlement may never price itself.** The marker consumed to
price — or to `price_dev_bound`-check — a settlement at Bitcoin
height `H` MUST be computed over a window ending strictly before `H`
(≥ 1 block lag). Otherwise a settlement that references
`internal_twap` (the eventual `oracle_spec = internal_twap` rails)
would depend on its own price. The lag breaks the self-reference.

**D — The external source is itself a competitive, curated layer.**
`P_external` (§5.1 / §5.5) is no privileged singleton: anyone may run
a competing external marker (the §5.5 stake-weighted median being the
RECOMMENDED form), rails name their chosen source in disclosure
(02 §6.4), and curation lists judge it like any other choice. The
external input obeys the same "trust priced by a market" rule as the
rails it anchors.

**On concentration — deliberately NOT a hard cap.** A single rail
holding a large share of `internal_twap` weight is acceptable **when
that share is costly-signal-backed** (rule A): genuine deep liquidity
is *where the truest price forms*, and capping it would discard real
price discovery and penalise honest depth. No-profitable-rug
(02 §6.2, α < 1) already makes abusing such a position unprofitable
*in-protocol*. The residual is narrow — a volume-dominant rail *sets*
the trimmed median (§5.2) and could skew it within the
authority-widened band to profit on **external** positions the
in-protocol accounting misses. That residual is bounded by the §5.5
cost floor (`sunk cost > external manipulation payoff, incl. shorts`),
**not** by a concentration cap, and is a named scenario the §6
adversarial simulation MUST exercise before any non-degenerate `w`
activates.

## 6. Open items

1. Exact CET wiring of the tlock escape (extend proposal 0001).
2. Adaptor-signature count economics for k-of-n beyond (3,5).
3. Whether marker attestations should also be dlcspecs
   announcements (R-committed) so DLC rails can settle directly
   against the marker — attractive; needs nonce-volume analysis.
4. internal_twap manipulation analysis at low volume — the
   min_volume_usd floor and the 5 % trim need adversarial
   simulation before any rail declares the class.
5. **The CtM estimator** (§5.4): a robust, un-gameable measure of
   internal cost-to-manipulate from S3 + retained-fee history
   (defensible *depth*, not volume) — the input to `w(h)`. Until it
   is pinned, `w` is the §5.3 step (binary degenerate).
6. **The `band(h)` widening schedule + reflexivity damping** (§5.4):
   adversarial simulation of a thin-market capture attempt under
   premature internal weight; the band's asymptotic circuit-breaker
   value.
7. **The §5.5 stake-weighted median**: bond sizing vs total
   extractable value (incl. external SatUSD shorts), the
   volatility-adaptive band width, commit-reveal transport, and
   reward funding before fee volume exists (subsidy vs Sybil-farm).
8. **Costly-backed concentration** (§5.6): the adversarial sim MUST
   exercise a volume-dominant, capacity-backed rail *setting* the
   trimmed median and skewing it within the widened band for
   external-value profit — to decide whether the §5.5 cost floor
   suffices or a soft (non-hard-cap) safeguard is warranted. A hard
   concentration cap is rejected (it would discard real depth).
