# SatUSD Reserve & Epoch Allotment

- **Spec**: 04
- **Version**: 0.0-skeleton — structure and commitments only;
  normative content to be filled as Rail-0/Rail-1 implementations
  inform it
- **Authority**: `docs/MISSION.md` v2 via ADR-0001; capacity
  mechanism per ADR-0002

## 1. Role of the reserve

The reserve is the bitcoin backing all circulating SatUSD. It has
exactly one outflow: **reimbursement** — an LP who fronted BTC to
a user and can prove the corresponding SatUSD burn (S3 artifacts,
spec 02 §4) claims the burned face value from the reserve, minus
the retained-fee haircut:

```
reimbursement = burned_amount × (1 − retain_bps) at the
                reimbursement price (§4)
```

The retained difference stays in the reserve permanently, raising
the collateral ratio for all holders. This haircut is the costly
signal feeding the capacity formula (spec 02 §6.1).

Mint direction (BTC inflow against SatUSD issuance) is governed by
the CR rules of §5 (TBD).

## 2. Epoch allotment

The epoch (2016 blocks ≈ 2 weeks, parameter pinned here) is the
cadence at which rail capacities are recomputed and reserve
tranches restructured. Capacity changes over time; enforcement
structures are static once erected; the epoch is the refresh
rhythm.

Each epoch boundary:

1. Compute `capacity(rail, epoch)` for every rail from public data
   (spec 02 §6.1).
2. Allocate per-rail tranche UTXOs sized by capacity. If the sum of
   capacities exceeds the reserve, every tranche scales
   **proportionally, floored** — deterministic and
   ordering-independent, so no rail is favored and the total never
   exceeds the reserve.
3. Unspent tranches timelock back to the general reserve at epoch
   end (consensus-enforced, Tier-1 automation).

A rail physically cannot draw beyond its tranche.

The Stage-1 plan computation is implemented (`satusd-allot`): pure
function from sorted rail disclosures to tranches, with an input
hash and a canonical plan hash — a third party re-running on the
same data reproduces the plan byte-for-byte (PRD FR-6 acceptance).

## 3. Three-stage enforcement hardening

| Stage | Trust window | Mechanism |
|---|---|---|
| 1 — manual | the whole epoch | Founder runs the open-source allotment script; publishes the plan (signed, with input-data hash); anyone re-runs to verify; deviation = public challengeable evidence. **Scaffolding** — removal criterion: Stage 2 ceremony operational |
| 2 — pre-signed ceremony | boundary only | Epoch-boundary ceremony pre-signs the next epoch's tranche tree: reimbursement paths behind optimistic challenge windows (Clementine-style kickoff/challenge), timeout return paths. Over-cap spends were never signed → impossible intra-epoch |
| 3 — covenant/BitVM | none | Formula + burn-proof verification enforced in spend conditions; ceremony retired. Upstream-dependent (CTV/CSFS or BitVM maturity) |

All three stages run the same formula on the same cadence; the
transition swaps only what guarantees execution.

## 4. Reimbursement pricing (open — binding)

The price at which a burned $X face value converts to sats at
reimbursement is **the** remaining oracle-dependence of the
reserve. Design space recorded:

- (a) the rail's settled price, REQUIRED within `price_dev_bound`
  of the epoch reference marker; or
- (b) the reference marker itself at the settlement block, with
  the LP's spread being the difference vs. what they paid the
  user.

(b) concentrates all price trust in the marker; (a) distributes it
but complicates the harm model. Decision deferred until Rail-0/1
implementations exercise both. The reference marker's source set
and cadence are owned by spec 03; its end state is the
`internal_twap` class (mission criterion applies).

## 5. To be specified

- CR rules and mint-side constraints
- Reserve key structure for Stage 1/2 (and its scaffolding entry)
- Tranche UTXO script templates per settle_primitive
- Challenge economics for Stage-2 optimistic reimbursement
- Reference marker interface (with spec 03)
