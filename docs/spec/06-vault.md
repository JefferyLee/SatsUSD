# SatUSD Vault Construction (mint-as-position-opening)

- **Spec**: 06
- **Version**: 0.1 — draft; vault funding output + checkpoint-CET
  liquidation + maturity/roll normative; parameters are conservative
  starting values (§8); M-C/M-D-era, NOT on the M-B critical path
- **Authority**: `docs/MISSION.md` v2 via ADR-0001; vault direction
  per ADR-0004; CR rules per spec 04 §5; DLC funding output per
  proposal 0001; oracle per spec 03; capacity per ADR-0002

## 1. Role of the vault

A **vault** is a **fixed-maturity DLC collateral primitive**: a party
locks its own BTC in a co-controlled DLC funding output (§2) and
pre-signs the paths that settle it (铸造即开仓). The vault is
mechanism; *who opens it and what it backs* gives two distinct uses
(ADR-0005):

- **LP-issuer sourcing collateral `Q` (the v0 path, spec 07 §2).** An
  LP opens a vault to lock the over-collateralised BTC that backs the
  redemption-bearing notes it sells; each note redeems **unilaterally**
  against that `Q` (spec 07 §3). The vault is the LP's supply-side
  inventory — the **end holder never opens one; they buy a note**.
- **CDP self-minter (the ADR-0004 vision; deferred).** A HODLer locks
  own BTC and mints against it ("spend dollars without selling
  bitcoin"), with a **common reserve** as clearing counterparty. This
  reserve-coupled flow — the diagram below, and the founder-reserve
  removal criterion (PRD §8, restated by ADR-0005 against *held*, not
  "circulating", SatUSD) — belongs to the **deferred reserve / covenant
  era** (spec 04 scope note): shared pools return with the covenant
  capability.

The reserve-coupled clearing model (the rest of this spec, §3–§9)
describes that deferred use; the v0 LP path reuses only the §2 funding
output + the §4 CET primitive, settling against notes rather than a
reserve:

```
DEFERRED (reserve / covenant era — ADR-0005, spec 04):
mint side                clearing layer          redeem side
vaults (anyone)   ──→     common reserve   ──→    rails (spec 02)
lock own BTC,             receives face value     holders burn
mint SatUSD               from vault settlement    SatUSD for BTC
```

A vault reuses, unchanged, the proposal-0001 construction (a P2TR
output that is simultaneously a DLC funding output and — where
needed — a TA-committed output) and the Rail-1 CET machinery (spec
02 §7: 2^m aligned price buckets, one oracle-anticipated adaptor
signature each). The differences from a Rail-1 redemption are three:
the counterparty is the **reserve**, not an LP; the pre-signed CETs
are **crash buckets only** (checkpoints, not a full settlement
spectrum); and value flows the other way — BTC in, newly-issued
SatUSD out.

This answers the strongest prior-art objection (ADR-0004 §Context):
the over-collateralized long side and the collateral source are the
**same actor** — a HODLer spending dollars without selling bitcoin.

## 2. Vault contract and funding output

A vault is a single DLC funding output holding the minter's BTC
collateral, co-controlled by the minter and the reserve:

```
internal key  P  = MuSig2_KeyAgg(minter_pubkey, reserve_pubkey)
script tree   T  = { refund_leaf }
                   refund_leaf = <CSV maturity+grace> 2-of-2
output key    Q  = P + TaggedHash("TapTweak", P ‖ MerkleRoot(T)) · G
value         C  = collateral in sats
```

- **2-of-2(minter, reserve)** (decision): during the scaffolding era
  there is no covenant to bind the collateral, so the funding output
  is a MuSig2 2-of-2. The reserve co-signs at open the entire
  pre-signed set (every checkpoint CET, the maturity CET, the burn-
  reclaim path); thereafter the minter **cannot unilaterally
  withdraw** before maturity except along a pre-signed path. The
  reserve key is itself a scaffolding entry — Stage 3 (§9) retires it
  for a covenant. The minter likewise cannot be rugged: every spend
  the reserve can reach is one the minter pre-signed.
- **Pre-signed from birth.** The construction *is* a one-shot
  pre-signing ceremony; a vault is Stage-2-grade (spec 04 §3) on day
  one. There is no manual stage to retire.
- **vault_id.** The canonical contract identifier is
  `vault_id = TaggedHash("SatUSD/vault-contract/v1", encode(terms))`
  over the terms: collateral `C`, mint amount `$X`, opening CR,
  oracle event series, checkpoint schedule, maturity height, the
  CET-set merkle root, and `Q`. A vault is referenced by `vault_id`
  the way a rail is by `rail_id`; parameter changes are a **new
  vault_id**, never a retroactive edit (spec 04 §5 discipline).

The minted SatUSD is issued to the minter's own TA address by a
separate issuance (spec 01 §3), not committed in the funding output;
the funding output holds only the BTC collateral and its settlement
paths.

## 3. Open (mint)

To open a vault for `$X` SatUSD against `C` sats at reference price
`P` (spec 03):

1. The minter funds `Q` with `C` sats and co-signs, with the
   reserve, the full pre-signed set (§4, §5).
2. The mint MUST satisfy the **minimum opening CR** (§8):
   `C × P / $X ≥ opening_CR`. Opening is **refused** in the
   PauseMint and below tiers (spec 04 §5): when system CR < 140% no
   new vault opens.
3. A **mint fee** (§8) is retained into the common reserve, raising
   CR for all holders (the same costly-signal haircut as
   reimbursement, spec 04 §1).
4. Conditioned on a valid vault — collateral verified, the complete
   crash-CET set verified pre-signed and correctly addressed — the
   group key signs the TA issuance of `$X` to the minter. This
   *is* the "spec-04 evidence chain" for the mint direction (spec 01
   §3, spec 02 §3.6): issuance is impossible without a standing,
   fully-pre-signed vault behind it. During scaffolding the group
   key is founder-held (PRD §8); Stage 3 gates issuance on the
   vault's on-chain construction directly.

Supply is tracked by the per-epoch TA supply commitment (spec 01
§5): each open adds `$X` to committed circulating supply, verifiable
by any observer, and counts in `face_supply` for System CR (spec 04
§5).

## 4. Checkpoint CETs and liquidation

There is no liquidation engine. At open, the parties pre-sign
**crash-bucket-only** CETs for each checkpoint (every `N` blocks
until maturity, §8), each an adaptor signature anticipating the
oracle attestation at that checkpoint's event (spec 03 §3.3, spec 02
§7). Each CET's adaptor lock is that bucket's **`crash_adaptor_point`**
— the oracle anticipation point `S = R + e·P` for the checkpoint event
and price bucket (spec 03 §3); the matching attestation decrypts
exactly that CET. (spec 07 §3.2 reuses this named point for the
unilateral **redemption** CETs an LP-issuer pre-signs over the same
`Q`; redemption CETs and these liquidation crash CETs coexist on one
`Q` — their non-overlap is a spec 07 §10 integration item.)

- A **healthy price** at a checkpoint corresponds to *no pre-signed
  transaction at all* — the buckets above the liquidation threshold
  were never signed, so nothing is broadcastable and the vault
  glides on.
- A price in a **crash bucket** — one where the vault's own CR has
  fallen to the **liquidation CR** (§8) — makes exactly that
  bucket's CET decryptable from the oracle's published signature.
  **Anyone** may decrypt and broadcast it for a bounty; liquidation
  is an anyone-can-act market role, like challengers and broadcasters
  (spec 02 §5 tier 2), never a privileged keeper.

A broadcast crash CET spends `C` to:

| Output | Recipient | Value |
|---|---|---|
| reserve | common reserve | `⌈$X / p_i⌉` (face) `+ penalty − bounty` |
| broadcaster | the broadcaster | `bounty` (§8) |
| minter | minter's address | remainder `C − face − penalty` |

The minted SatUSD stays in circulation; the crash CET converts the
collateral into reserve backing of that face value (this is why
spec 04 §5 counts vault collateral in System CR). The minter keeps
the SatUSD they minted and the collateral above face+penalty — the
liquidation is the loss of the over-collateralized cushion plus the
penalty, not a total loss. **Gap risk** (price gapping below the
crash bucket between checkpoints) is conceded and priced ex-ante
(ADR-0004 §3): checkpoint spacing `N` and the opening CR bound the
absorbable gap; beyond it the loss lands on the pooled cushion, then
on the NAV floor (spec 04 §5). The protocol does not pretend gap
risk away.

## 5. Maturity, burn-reclaim, and rolling

At the maturity height the minter has two clean exits:

- **Burn-reclaim** (the healthy close): the minter burns `$X`
  SatUSD (any rail's burn artifact, or a direct TA burn, spec 01 §4),
  proves it, and reclaims **all** collateral `C` along the
  pre-signed burn-reclaim path. Supply conservation holds (spec 00
  §4 S2): the minted dollars are destroyed exactly as the collateral
  is freed.
- **Maturity CET**: if the minter does not burn, the pre-signed
  maturity CET settles — face value `⌈$X / P_maturity⌉` to the
  reserve, excess back to the minter. The circulating SatUSD becomes
  reserve-backed, identically to a crash settlement.

**Rolling** is closing + reopening: a new vault_id with a fresh
schedule, plus an explicit **roll fee** retained to the reserve. The
carry cost of a collateralized dollar position exists, is priced,
and accrues to the commons rather than to an exchange or a lender.

## 6. Oracle liveness (minter-safe + tlock)

If the oracle silences during a vault term, crash-bucket CETs cannot
be decrypted (their adaptor secret is the missing attestation). The
risk is allocated **minter-safe** (decision):

- While the oracle is silent, **no CET is broadcastable** — the
  vault cannot be liquidated. The minter is never punished for the
  oracle's failure (consistent with spec 04 §5: redemption/closing
  is always the healing mechanism, never a threat).
- The **reserve bears** any gap that opened during the silence — the
  same pooled cushion that absorbs inter-checkpoint gap (§4). An
  oracle going dark is, for the reserve, a maximally adverse gap.
- A **tlock backstop** (spec 03 liveness escape) on the `refund_leaf`
  CSV lets the minter reclaim collateral after a maximum-silence
  timeout, so a dead oracle can never strand collateral. The timeout
  is set well beyond normal maturity (§8).

This asymmetry — minter protected, reserve exposed — is deliberate:
oracle marketization (spec 03) makes oracle silence a *competitive*
failure the reserve can price and route around, while a minter
punished for it would have no recourse.

## 7. Collateral-ratio contribution and gates

A vault interacts with the System CR of spec 04 §5 in two directions:

- **Contribution.** `Σ vault_collateral_sats` enters the System CR
  numerator, because crash and maturity CETs deliver face value to
  the reserve; vault collateral is reserve-grade up to the residual
  gap risk the tier parameters bound.
- **Gating.** Opening obeys the system tiers: refused at PauseMint
  (system CR 120–140%) and below; in Recovery (100–120%) the minimum
  opening CR rises (§8). A vault's **own** CR (`C × P / $X`) is what
  the checkpoint schedule watches for liquidation; the **system** CR
  is what gates new opens. The two are distinct and both public —
  FR-7 clients MUST compute both from chain data and disclosures
  alone.

## 8. Parameters

All values below are **conservative starting values, tuned by data**
(spec 04 §5). Changes apply **forward only** — a new vault_id, never
a retroactive edit to a standing vault.

| Parameter | Symbol | Starting value | Notes |
|---|---|---|---|
| Minimum opening CR | `opening_CR` | 150% | rises in Recovery tier (spec 04 §5) |
| Liquidation CR | `liq_CR` | 110% | crash buckets are prices where own CR ≤ this |
| Checkpoint interval | `N` | 144 blocks (~1 day) | smaller `N` ⇒ tighter gap bound, more CETs |
| Mint fee | `mint_bps` | 50 bps of `$X` | retained to reserve at open |
| Liquidation penalty | `penalty_bps` | 500 bps of `$X` | to reserve (less the bounty) |
| Broadcaster bounty | `bounty` | min(10% of penalty, a sat cap) | the anyone-can-act incentive |
| Roll fee | `roll_bps` | 25 bps of `$X` | retained to reserve on roll |
| Max oracle-silence timeout | `tlock_max` | maturity + 4032 blocks (~4 wks) | refund_leaf CSV backstop (§6) |
| CET bucketization | `m` | per spec 02 §7 | crash region only needs the low buckets |

`opening_CR > liq_CR` MUST hold by construction; the gap between
them is the cushion a vault opens with before any checkpoint can
liquidate it.

## 9. Three-stage hardening

The vault inherits spec 04 §3's hardening ladder:

| Stage | Vault form | Scaffolding |
|---|---|---|
| 2 — pre-signed ceremony | 2-of-2(minter, reserve) MuSig2; reserve co-signs the full CET/maturity/reclaim set at open | the **reserve key** is the scaffold; removal criterion: Stage 3 covenant operational |
| 3 — covenant | collateral bound by a covenant (CTV/CSFS or BitVM); issuance gated on the on-chain vault directly; the reserve co-signing key retired | upstream-dependent |

Both stages run the identical economic contract; Stage 3 swaps only
what *binds* the collateral and *authorizes* the issuance, removing
the founder/reserve key from the trust path. Per PRD §8, the
founder-funded reserve itself is removed once **vault-minted supply
≥ 50% of circulating SatUSD**.

## 10. Implementation status

Implemented in `crates/satusd-vault` (BTC-layer math + the crash-CET
schedule), reusing the rail-1 adaptor/CET/funding primitives. 13 unit
tests pin the funding-output reconstruction, CR/opening arithmetic,
crash payout (including the gap clamp), and the crash-bucket schedule;
the `vault_id` commitment is deterministic.

Both settlement outcomes are **devnet-validated** on live regtest
(`crates/satusd-vault/tests/devnet_vault.rs`, the way proposal 0001 §8
validated redemption); the vault collateral being pure BTC, the CETs
need no tapd machinery:

- **Crash** (`vault_lifecycle_crash_settle`): a vault opens (collateral
  locked at Q, scriptPubKey = the reconstructed output key), every
  crash bucket's CET is pre-signed before the outcome, a healthy price
  falls in no pre-signed bucket (glide), and an oracle attestation at a
  crash price decrypts exactly that bucket's CET — a key-path spend
  broadcast and confirmed, with face+penalty→reserve, bounty→broadcaster,
  cushion→minter on-chain.
- **Healthy close** (`vault_burn_reclaim`): a vault opens and is
  reclaimed in full to the minter by a single key-path spend of Q (the
  burn-reclaim path, §5) — no oracle.
- **Oracle-silence backstop** (`vault_tlock_refund`): a vault opens and
  is reclaimed via the refund_leaf **script-path** after the CSV tlock
  matures (§6) — the only script-path spend of Q; the minter is never
  stranded by a dead oracle.
- **Issuance at open** (`vault_open_and_mint`): a grouped SatUSD asset's
  group key reissues `$X` into the group against an open vault, growing
  the group's committed supply by exactly the minted amount (§3 step 4)
  — the vault is a real CDP: lock BTC, mint SatUSD. v0 uses one tapd as
  both the founder group-key holder (scaffolding) and the minter.
- **MuSig2(minter, reserve)** (`vault_musig2_reclaim` + the `musig`
  unit tests): the funding output's internal key is the real BIP-327
  aggregate of the minter and reserve keys (via the audited `musig2`
  crate — `secp256k1` 0.31 still has no musig module), tweaked to Q; the
  reclaim is a genuine 2-of-2 key-path co-signature, verified under Q by
  the project's own secp256k1 and broadcast on-chain. This replaces v0's
  single funding key for the §2 construction and the key-path reclaim.

Validated: §2 funding output (incl. MuSig2 aggregate key), §3 opening
CR + issuance, §4 crash settlement, §5 burn-reclaim, §6 tlock refund.
Not yet exercised end-to-end (follow-ups, §11): the §5 maturity CET (an
oracle-gated settle at the maturity event, mechanically the crash CET);
the **MuSig2-adaptor crash CET** (the §4 crash path still pre-signs with
the single-key adaptor; the 2-of-2 adaptor co-sign is `musig2`'s
`finalize_adaptor`); the §9 reserve ceremony.

## 11. To be specified

- Exact crash-bucket → price-region mapping (which spec 02 §7 buckets
  are pre-signed for a given `opening_CR`/`liq_CR`/`N`), and the
  per-checkpoint event-series binding (spec 03).
- Vault PSBT proprietary fields (extending proposal 0001 §4:
  `vault_cet/<checkpoint>/<bucket>`, `vault_maturity`,
  `vault_reclaim`, `vault_terms`).
- Reserve co-signing ceremony for opens (relationship to the spec 04
  §3 Stage-2 epoch-boundary ceremony — per-vault vs batched).
- Reserve key structure (shared with spec 04 §6 "reserve key
  structure"); FROST k-of-n vs single during scaffolding.
- Devnet validation of a full vault lifecycle (open → checkpoint
  glide → crash settle and burn-reclaim), the way proposal 0001 §8
  validated the redemption settlement.
- Interaction with epoch allotment (spec 04 §2): whether vault
  issuance draws on an epoch tranche or is capacity-independent.
