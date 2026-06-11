# SatUSD Oracle Classes & the Reference Marker

- **Spec**: 03
- **Version**: 0.1-draft
- **Authority**: `docs/MISSION.md` v2 via ADR-0001
- **Depends on**: 00 (conventions), 02 (manifest `oracle_spec`),
  dlcspecs (Oracle.md, Messaging.md, NumericOutcome*)

Defines the four oracle classes a RailManifest may declare, the
attestation formats, equivocation handling, liveness escapes, and
the **reference marker** — the protocol-level price every other
spec leans on. The mission criterion governs throughout: each class
exists to be outcompeted by the one below it on the list.

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
inputs:  all S3 settlement artifacts for the asset within
         window_blocks (default 144) ending at height h
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

## 6. Open items

1. Exact CET wiring of the tlock escape (extend proposal 0001).
2. Adaptor-signature count economics for k-of-n beyond (3,5).
3. Whether marker attestations should also be dlcspecs
   announcements (R-committed) so DLC rails can settle directly
   against the marker — attractive; needs nonce-volume analysis.
4. internal_twap manipulation analysis at low volume — the
   min_volume_usd floor and the 5 % trim need adversarial
   simulation before any rail declares the class.
