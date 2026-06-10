# §13 Adversarial Coverage Ledger

Maps every PRD §13 adversarial case to where it is enforced/tested, or marks it
deferred. Status legend: **✓ code** = enforced + unit/integration-tested;
**✓ devnet** = demonstrated by a devnet bin (app-layer, not a unit test);
**~ partial**; **⏸ deferred** = needs on-chain / signet / upstream layer not built;
**social** = detection/social-layer (challenger alert), not a consensus check.

Cross-checked against the test suite (177 tests) + the verifiers; cites the
covering module (not always the exact fn).

## 13.1 Redemption (R-01..15)

| ID | Scenario | Status | Where |
|---|---|---|---|
| R-01 | operator never sends HTLC → user refunds | ✓ devnet | `satusd-tapd-client::refund_monitor` (CSV refund on devnet) |
| R-02 | HTLC amount too low → user won't claim | ✓ devnet | operator BTC-leg (`operator_demo`); user-side accept logic |
| R-03 | wrong user_claim_pubkey | ✓ devnet | HTLC leaf construction (`satusd-operator::htlc`) |
| R-04 | wrong payment_hash | ✓ devnet | HTLC leaf (`htlc`); preimage↔hash bound |
| R-05 | insufficient BTC CSV (< asset CSV) | ✓ code | intent/HTLC CSV check |
| R-06 | user never claims → operator refunds | ✓ devnet | `refund_monitor` + HTLC refund leaf |
| R-07 | mempool-only preimage (depth < 6) | ✓ code | `satusd-state::spv` (DL-22) |
| R-08 | stale oracle epoch | ✓ code | `satusd-state::oracle` freshness + ClaimClock (in `claim`) |
| R-09 | double-finalize same lock | ✓ code | `satusd-state::{claim,node}` lock_consumed non-membership (D17) |
| R-10 | two locks from one intent | ✓ code | redemption nullifier (`claim`/`redeem`) |
| R-11 | bind different intent to same lock | ✓ code | `derive_lock_script_key` binding (`redeem::check_intent_lock_binding`) |
| R-12 | mismatched preimage | ✓ code | `satusd-state::claim` (PreimageMismatch) |
| R-13 | confirmation_headers.len() < 6 | ✓ code | `satusd-state::spv` |
| R-14 | bad SPV merkle proof | ✓ code | `satusd-state::spv` |
| R-15 | claim_spend ≠ HTLC outpoint | ✓ code | `satusd-state::spv` (R-15, `claim_tx_legacy`) |
| (R-16) | burn output not to NUMS sink | ✓ code | `satusd-state::claim` (extra; BurnProofScriptKeyMismatch) |
| (R-17) | finalize-then-refund | ✓ code | `satusd-state::redeem` (extra) |

## 13.2 Oracle (O-01..08)

| ID | Scenario | Status | Where |
|---|---|---|---|
| O-01 | quorum < 3 | ✓ code | `satusd-state::oracle` |
| O-02 | wrong epoch signed | ✓ code | `oracle` |
| O-03 | timestamp too old | ✓ code | `oracle` freshness |
| O-04 | timestamp too far future | ✓ code | `oracle` freshness |
| O-05 | price outlier > 5% (median ±2% inliers) | ✓ code | `oracle` aggregate (per §5.D7 errata) |
| O-06 | one signer chronically wrong | **social** | challenger alert (source transcripts in DA); not a consensus check |
| O-07 | forged EdDSA-BabyJub sig | ✓ code | `satusd-crypto::eddsa` + `oracle::verify_signatures` |
| O-08 | signer not in oracle set | ✓ code | `oracle` (set-membership) |

## 13.3 Issuer (I-01..07)

| ID | Scenario | Status | Where |
|---|---|---|---|
| I-01 | deposit unconfirmed at MINT_COMMIT | ✓ code | `satusd-state::mint` |
| I-02 | deposit to non-reserve address | ✓ code | `mint` (DepositNotToReserve) |
| I-03 | finalize anchor ≠ commit | ✓ code | `mint` (pending_mint_commitment, ADR-0019) |
| I-04 | CR ok at commit, < 150% at finalize | ✓ code | `mint` (DL-27 re-check, ADR-004) |
| I-05 | FROZEN issuer mints | ✓ code | `mint` |
| I-06 | multisig 1-of-3 | ✓ code | `mint` + `satusd-crypto::ecdsa` |
| I-07 | double-finalize same commit | ✓ code | `mint` |

## 13.6 Mint/Tier (T-01..04)

| ID | Scenario | Status | Where |
|---|---|---|---|
| T-01 | CR < 150% at MINT_FINALIZE | ✓ code | `mint` (= I-04 / DL-27) |
| T-02 | CR < 130% any mint | ✓ code | `satusd-types::tier` thresholds + `mint` |
| T-03 | tier-3 redeem → manual stop | ~ partial | `tier` band (Settlement); automatic-stop policy is operational |
| T-04 | tier recompute matches oracle price | ✓ code | `tier` (G3: 327 fixtures, 3-way) |

## 13.7 Burn / Lineage / Confirmation (BLC-01..06)

| ID | Scenario | Status | Where |
|---|---|---|---|
| BLC-01 | finalize to non-NUMS sink | ✓ code | `satusd-state::claim` (D16 sink derivation) |
| BLC-02 | missing universe burn proof | ✓ code | `claim` (burn_proof now real, not a fact) |
| BLC-03 | burn proof ≠ finalize anchor | ✓ code | `claim` (AnchorMismatch) |
| BLC-04 | forged TA lineage | ✓ code | `satusd-state::redeem` (DL-23) + `satusd-dispute` (resolve) + `satusd-challenger` |
| BLC-05 | lineage ≠ lock anchor outpoint | ✓ code | `redeem` (§5.D15 binding) |
| BLC-06 | header chain not best chain | ✓ code | `satusd-state::spv` (PoW + chain) |

## 13.4 DA (DA-01..06)

| ID | Scenario | Status | Where |
|---|---|---|---|
| DA-01 | no DA uploaded | ✓ code | `satusd-challenger::inspect` |
| DA-02 | bundle ≠ committed root | ✓ code | `challenger` |
| DA-03 | missing oracle section (0x08) | ✓ code | `challenger` |
| DA-04 | archival retrieval fails | ⏸ deferred | multi-mirror fetch (M5 ops, not built) |
| DA-05 | mirrors disagree | ⏸ deferred | multi-mirror selection (M5 ops) |
| DA-06 | missing lineage section | ✓ code | `challenger` (maps to 0x07; see ADR-008) |

## 13.5 Reserve (RB-01..08)

| ID | Scenario | Status | Where |
|---|---|---|---|
| RB-01 | operator abandons, CSV expires → keeper reclaim | ~ partial | `satusd-state::registry` reclaim_stale + `reserve` reclaim; keeper automation deferred |
| RB-02 | committee 1 refuses (3-of-5 still met) | ✓ code | `satusd-reserve::MultisigReserve` (threshold) |
| RB-03 | committee 3 refuse → stuck + review | ✓ code | `MultisigReserve` (ApprovalInsufficient) |
| RB-04 | Disprove tx fee too low → CPFP | ⏸ deferred | M7 optimistic on-chain |
| RB-05 | Assert tx RBF'd | ⏸ deferred | M7 |
| RB-06 | BitVM3 ceremony mis-sign | ⏸ deferred | M8 |
| RB-07 | Disprove in last 10s of window | ⏸ deferred | M8 |
| RB-08 | reorg drops Assert tx | ⏸ deferred | M7/M8 |

## Summary

- **Enforced + tested in code (✓ code):** all of R-07..15 (+R-16/17), O-01..05/07/08,
  I-01..07, T-01/02/04, BLC-01..06, DA-01/02/03/06, RB-02/03 — the full
  consensus-relevant verifier surface.
- **Demonstrated on devnet (✓ devnet):** R-01..06 (operator BTC-leg + refund), via
  `operator_demo` / `refund_monitor`.
- **Partial / operational:** T-03 (automatic-stop policy), RB-01 (keeper automation).
- **Social / detection:** O-06 (challenger alert, not a consensus check).
- **Deferred (need on-chain / signet / multi-mirror, not a code gap):** DA-04/05
  (multi-mirror), RB-04..08 (optimistic/BitVM3/reorg).

No consensus-relevant case is unenforced. The deferred set is uniformly the
live-infra / on-chain-enforcement layer tracked in `SECURITY.md` §3.
