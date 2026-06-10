# SatUSD — Security Model & Trust Assumptions

This document states, for an external reviewer, **what SatUSD currently guarantees,
what it trusts, and what is deferred.** It is deliberately conservative — the point
is not to oversell. Authoritative details live in `docs/SatUSD_v5_1_PRD.md` (+ its
errata table) and `docs/decisions/` (ADRs).

> **Stage.** This is a **solo / AI-assisted development build**, conditionally
> approved for M0/M1 only (see the PRD header). The protocol's M0–M8 + G1–G4
> *code cores* are implemented and tested in a **regtest devnet**; the **signet
> pilot, mainnet, real BTC, BitVM3 on-chain enforcement, and an external audit are
> NOT done.** Do not use with funds at risk.

## 1. Roles & trust assumptions (current MVP)

| Role | What it does | Trusted to… (MVP) | De-trusted by… |
|---|---|---|---|
| **State node** | executes transitions, holds the StateRoot | be correct | M6 dual independent instances (byte-identical StateRoot) + M5 challenger recompute |
| **Reserve committee (3-of-5)** | approves reserve payout (FINALIZE_CLAIM) | not collude; honor veto/pause | M8 BitVM3 trustless reserve (committee retired) |
| **Issuer multisig** | authorizes mint | not mint without deposit (I-01..07 enforced) | — (issuer is permissioned) |
| **Oracle set (3-of-5)** | signs BTC/USD price | quorum honest, no >2% outlier collusion | median + ±2% inlier rule; source transcripts in DA for recompute |
| **Operator** | funds BTC HTLC, submits claims | nothing — claims are fully verified | software verifier + challenger + (M8) on-chain dispute |
| **Challenger** | independently recomputes claims | ≥1 honest, online | anyone can run; detection now, on-chain Disprove at M8 |

**Bottom line (MVP):** the reserve's integrity rests on the **3-of-5 committee not
colluding**, plus ≥1 honest challenger + ≥2 independent state nodes (M6). BitVM3
(M8) replaces committee trust with on-chain optimistic enforcement; until then the
committee is the load-bearing trust assumption.

## 2. What is cryptographically enforced (in code, today)

All of these are real verifiers with tests (not trusted facts), cross-checked
Rust = TypeScript (`make verify`, 4481 assertions) and, for circuit gadgets,
= circom witness-calc (`circuits/check.sh`):

- **Canonical encoding + domain-separated hashing** (§18, ADR-0011) — `satusd-types`.
- **Poseidon / SMT** (BN254, circomlib-pinned; ADR-0013/0015) — `satusd-crypto`.
- **CR / tier** accounting (DL-24) — `satusd-types::tier` (327 fixtures, G3).
- **TA lineage** (DL-23, §5.D15): genesis→head proof + intent↔lock binding —
  `satusd-ta-proof`, `satusd-state::redeem` (real devnet vectors).
- **BTC SPV payout** (DL-22, §5.D14): merkle + PoW + depth + R-15 spend-of-HTLC —
  `satusd-state::spv`.
- **Universe burn** (D16): asset committed to the protocol NUMS sink —
  `satusd-state::claim`.
- **Oracle EdDSA-BabyJub** signatures + aggregation (§5.D7) — `satusd-state::oracle`.
- **Issuer / reserve-committee multisig** (secp256k1 ECDSA) — `satusd-crypto::ecdsa`.
- **Two-phase ReserveClaim** lifecycle + nullifier/lock-consumed sets (§5.D12/D17,
  ADR-0022/0023) — `satusd-state::{claim,registry,node}`.
- **Live DA bundle root** (§10.3, ADR-008) — `satusd-da`, recomputed by the
  challenger (`satusd-challenger`).
- **State commitment** = Poseidon-over-fields `state_root_hash` (ADR-010), with the
  in-circuit form (`m7_transition` `Fold(42)`) equal to it.

The **state node + challenger both run** the lineage and SPV verifiers
independently (§5.D18), so neither has to be trusted for those.

## 3. What is trusted / not-yet-enforced (honest gaps)

- **(closed)** ~~Reserve deposit was a witness fact at mint_commit.~~ As of the
  deposit-SPV upgrade, `mint_commit` independently re-verifies the deposit on
  Bitcoin: tx body → txid, an output pays the committee P2WSH for `deposit_sats`,
  merkle inclusion + PoW + ≥ K-deep confirmation chain. The same machinery as the
  redemption SPV (DL-22). Issuer + operator build the proof via
  `satusd_operator::build_deposit_confirmation`. The full regtest E2E exercises it.
- **The succinct proof is NOT consensus-authoritative yet.** The MVP proves
  transitions with the **software verifier**; the challenger re-runs it. The Groth16
  circuit is supplementary:
  - The full Groth16 *pipeline* is proven (`prove.sh`, `m4a_cr_tier`, `snarkJS: OK!`).
  - `m7_transition` is proven **satisfiable** with the real witness (3-way
    witness-calc), and after ADR-010 its commitments equal the on-chain
    `state_root_hash` — but it covers only a **subset** of the transition rules
    (lock state machine + supply/epoch), not the full transition; and an actual
    Groth16 prove/verify of m7 needs a 2^21 ptau (environment-bound here — see
    `circuits/prove_m7.sh`). A single succinct proof that *is* the consensus check
    remains future work.
- **The challenger is detection-only.** It recomputes + alerts/vetoes; **on-chain
  enforcement (Disprove) is M8/BitVM3** and not deployed. Today a fraudulent claim
  is caught by the challenger + committee manual review/veto, not by an automatic
  on-chain slash.
- **BitVM3 dispute is a software model**, not on-chain garbled circuits. The
  lineage / lock-binding dispute *logic* is implemented + tested (`satusd-dispute`,
  ADR-007); the on-chain tx graph, garbled SHA-256/secp256k1 sub-circuits, and
  setup ceremony are upstream-dependent (G6-gated, BitVM2 fallback per DL-33).
- **SHA-256 / secp256k1 / TapTweak stay off-circuit** (DL-25). They are enforced by
  the software verifier + recomputed by the challenger; in BitVM3 they move into
  dispute-only sub-circuits.
- **Reserve custody is modelled, not on-chain.** `satusd-reserve` tracks custody +
  committee approval in lockstep with state; the actual 3-of-5 P2WSH/P2TR reserve
  spend on signet is pilot work (M6).
- **Committee rotation is a stub, not on-chain enforcement.** If a
  Reserve committee member's key is lost or compromised, the
  `ROTATE_SHARD` transition (`0x50`, §11.2) marks the rotation in
  state, but the actual key exchange + custody handoff is **manual,
  off-chain**, and announced via the operator runbook
  (`docs/runbooks/M6_emergency_recovery.md`). The 3-of-5 multisig is
  **not immutable** — it can rotate — but until BitVM3 (M8) replaces
  the committee, rotation honesty rests on the same trust assumption
  as the committee itself.
- **Test fixtures are pinned to a devnet asset** — see
  `integration/lineage_vectors/PROVENANCE.md`; a devnet rebuild requires
  `make recapture-vectors` + constant updates (drift fails loudly).
- **No external audit, no bug bounty, no formal verification.** M6 starts the bounty.

## 4. Adversarial coverage

The §13 adversarial matrix (44 cases) is the acceptance contract. Status by class:

| Class | IDs | Where enforced |
|---|---|---|
| Mint/tier (T) | T-01..04 | `satusd-state::mint` + tier (G3) ✓ |
| Issuer (I) | I-01..07 | `satusd-state::mint` ✓ |
| Oracle (O) | O-01..08 | `satusd-state::oracle` ✓ |
| Redemption (R) | R-01..15 | `satusd-state::{redeem,claim,spv}` ✓ (incl. R-15 spend-of-HTLC) |
| Burn/Lineage/Conf (BLC) | BLC-01..06 | `satusd-ta-proof` + `satusd-state` ✓ (BLC-04 also at the dispute layer) |
| DA (DA) | DA-01..06 | `satusd-challenger` ✓ for DA-01/02/03/06; **DA-04/05 (multi-mirror) deferred** |
| Reserve (RB) | RB-01..08 | **deferred** — signet/optimistic/BitVM3 (M6/M7/M8) |

The full per-ID coverage ledger is `docs/discovery/adversarial_coverage.md`: every
consensus-relevant case is enforced + tested; the deferred set (DA-04/05 multi-mirror,
RB-04..08 optimistic/BitVM3) is uniformly the on-chain/live-infra layer.

## 5. Cryptographic primitives

- **Poseidon** over BN254, circomlib/iden3 parameterization (light-poseidon
  `new_circom`, circomlibjs reference) — ADR-0013.
- **Sparse Merkle Tree**, height 256, Poseidon nodes — ADR-0015.
- **EdDSA-BabyJubjub** (oracle), iden3/circom convention — verify-only on-chain.
- **secp256k1 ECDSA** (issuer / committee multisig), compact 64-byte sigs.
- **SHA-256** + tagged-hash domain separators (raw ASCII, no padding — ADR-0011).
- **NUMS / TapTweak** (BIP-341) for lock anchors + burn sinks — ADR-0014.

Every primitive is pinned Rust = TypeScript (and = circom where in-circuit), so a
parameter mismatch fails the cross-language gate rather than going unnoticed.

## 6. Reporting

This is a pre-production solo project; there is no bounty yet (M6). For now, raise
security concerns via the repository issue tracker / the maintainer. A responsible
disclosure policy + bounty will accompany the M6 signet pilot.
