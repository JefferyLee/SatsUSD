# Active ADR Index

| ADR | Title | Status |
|---|---|---|
| [ADR-001](ADR-001-lock-plan.md) | SatUSD lock plan (G1) — asset-layer anchoring path; G1 passed | accepted |
| [ADR-002](ADR-002-btc-payout-confirmation.md) | BTC payout confirmation requirements (G2); adds `claim_tx_legacy` for R-15 | accepted |
| [ADR-003](ADR-003-cr-formula.md) | Collateral-ratio formula final form (G3) | accepted |
| [ADR-004](ADR-004-mint-finalize-cr-recheck.md) | Mint finalize CR re-check on post-mint supply (G3 / DL-27) | accepted |
| [ADR-005](ADR-005-zk-toolchain.md) | zk toolchain — circom + circomlib + Groth16/BN254 | accepted |
| [ADR-006](ADR-006-circuit-state-commit-and-transition.md) | In-circuit state commit (StateCommit gadget) + monolithic transition deferred to M7 (Poseidon-of-fields) | accepted |
| [ADR-0011](ADR-0011-domain-separator-encoding.md) | Canonical hash domain-separator convention (raw ASCII, no padding) | accepted |
| [ADR-0012](ADR-0012-claim-id-preimage.md) | `claim_id` preimage excludes `claim_id` and `operator_signature` (amended by ADR-0022) | accepted |
| [ADR-0013](ADR-0013-poseidon-bn254.md) | Poseidon parameterization (circomlib BN254) + byte→field packing | accepted |
| [ADR-0014](ADR-0014-nums-and-taptweak.md) | NUMS key derivation and the TapTweak primitive | accepted |
| [ADR-0015](ADR-0015-smt-conventions.md) | Sparse Merkle tree conventions (height 256, Poseidon) | accepted |
| [ADR-0016](ADR-0016-enum-discriminants.md) | Status enum discriminants + FreezeReason variant set | accepted |
| [ADR-0017](ADR-0017-native-regtest-devnet.md) | Native-binary regtest devnet (interim, vs docker-compose) | accepted |
| [ADR-0018](ADR-0018-v52-bitvm2-fallback-and-covenant-gates.md) | v5.2 — BitVM2 fallback, advisory gates G5/G6, covenant risk | accepted |
| [ADR-0019](ADR-0019-issuer-pending-mint-commitment.md) | IssuerPosition `pending_mint_commitment` (two-phase mint linkage; I-03/I-07) | accepted |
| [ADR-0020](ADR-0020-spv-confirmation-fields.md) | BtcPayoutConfirmation SPV-completeness fields (tx index + inclusion headers) | accepted |
| [ADR-0021](ADR-0021-operator-position.md) | OperatorPosition struct + OPERATOR_REGISTER (bond ≥ 2× max claim) | accepted |
| [ADR-0022](ADR-0022-two-phase-claim-and-claimid-fixpoint.md) | Two-phase ReserveClaim (submit reserves, no reserve debit) + claim_id drops new_state_root | accepted |
| [ADR-0023](ADR-0023-reserve-committee-approval.md) | Reserve-committee 3-of-5 approval gates FINALIZE_CLAIM (`reserve_committee_hash` in StateRoot) | accepted |

Discovery reports: `docs/discovery/` — G1 (lock), G2 (BTC SPV payout), G3 (CR/tier
formula), M1 (BurnAsset), M4 (gate-count report). G1/G2/G3 all **passed**.

Reserved (placeholders to be filled later, per PRD §14):
ADR-007 (BitVM3 lineage dispute, M8).
