# SatUSD Reserve & Epoch Allotment

- **Spec**: 04
- **Version**: 0.1 — reimbursement pricing decided (§4); CR tiers,
  supply gates, and the NAV floor normative (§5); CDP-aware per
  ADR-0004
- **Authority**: `docs/MISSION.md` v2 via ADR-0001; capacity
  mechanism per ADR-0002; vault direction per ADR-0004

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

## 4. Reimbursement pricing (decided)

Reimbursement converts a burned $X face value to sats at **(a) the
rail's settled price**, which MUST lie within the manifest's
`price_dev_bound_bps` of the epoch reference marker. Rationale: the
deviation is bounded per settlement, the blast radius is bounded
per rail by capacity (ADR-0002), and trust stays distributed across
competing rails. The alternative — pricing every reimbursement at
the reference marker — would concentrate all price trust in one
input, against the oracle-marketization direction (spec 03). The
marker remains the *bound*, not the *price*; its end state is the
`internal_twap` class (mission criterion applies).

A settlement outside the bound is reimbursable at most at the
bound's edge; the difference is the LP's loss, not the reserve's
(rounding and bounds always favor the reserve, spec 00 §3.7).

## 5. Collateral ratio: tiers, gates, and the NAV floor

System CR is defined CDP-aware (ADR-0004):

```
CR = (reserve_sats + Σ vault_collateral_sats) × P / face_supply
```

where `P` is the epoch reference marker (spec 03) and
`face_supply` is the committed circulating supply (spec 01 §5).
Until vaults exist the vault term is zero. Vault collateral counts
because crash-bucket checkpoint CETs deliver face value to the
reserve (ADR-0004); the residual gap risk is what the tier
parameters bound.

| Tier | System CR | Gates (normative) |
|---|---|---|
| Healthy | ≥ 140% | none |
| PauseMint | 120–140% | mint-direction rails MUST refuse; new vaults MUST NOT open. Redemption is UNRESTRICTED at every tier — burning supply raises CR; redemption is the healing mechanism, never the threat |
| Recovery | 100–120% | PauseMint gates, plus: minimum vault opening CR rises and `retain_bps` floors rise for new manifests (accelerated recapitalization) |
| NAV floor | < 100% | redemption price = CR × face. Every holder redeems at the same published net asset value — no first-mover subsidy, no run race. The protocol never promises par; it promises seniority and an always-open exit at verifiable NAV |

Parameter discipline: all tier boundaries, the vault opening CR,
and checkpoint spacing are **conservative starting values, tuned by
data**. Changes apply forward only — new manifests, new vault
terms — never to standing positions (a parameter change is a new
rail_id / new vault contract, not a retroactive edit).

Recapitalization honesty: retained fees raise CR slowly (bps on
volume); the heavy lifting under stress is done by the gates
(supply stops growing) and by redemption burns (supply shrinks).
The cushion's depth is a public, verifiable number at all times —
FR-7 clients MUST be able to compute the tier from chain data and
disclosures alone.

## 6. To be specified

- Reserve key structure for Stage 1/2 (and its scaffolding entry)
- Tranche UTXO script templates per settle_primitive
- Challenge economics for Stage-2 optimistic reimbursement
- Reference marker interface (with spec 03)
- Vault construction precision (checkpoint CET schedules, bounty
  sizing, roll mechanics) — future spec 06 per ADR-0004
