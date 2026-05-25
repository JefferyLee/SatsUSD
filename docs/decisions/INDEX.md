# Active ADR Index

| ADR | Title | Status |
|---|---|---|
| [ADR-001](ADR-001-lock-plan.md) | SatUSD lock plan (G1) — asset-layer anchoring path; G1 passed | accepted |
| [ADR-0011](ADR-0011-domain-separator-encoding.md) | Canonical hash domain-separator convention (raw ASCII, no padding) | accepted |
| [ADR-0012](ADR-0012-claim-id-preimage.md) | `claim_id` preimage excludes `claim_id` and `operator_signature` | accepted |
| [ADR-0013](ADR-0013-poseidon-bn254.md) | Poseidon parameterization (circomlib BN254) + byte→field packing | accepted |
| [ADR-0014](ADR-0014-nums-and-taptweak.md) | NUMS key derivation and the TapTweak primitive | accepted |
| [ADR-0015](ADR-0015-smt-conventions.md) | Sparse Merkle tree conventions (height 256, Poseidon) | accepted |
| [ADR-0016](ADR-0016-enum-discriminants.md) | Status enum discriminants + FreezeReason variant set | accepted |
| [ADR-0017](ADR-0017-native-regtest-devnet.md) | Native-binary regtest devnet (interim, vs docker-compose) | accepted |
| [ADR-0018](ADR-0018-v52-bitvm2-fallback-and-covenant-gates.md) | v5.2 — BitVM2 fallback, advisory gates G5/G6, covenant risk | accepted |
| [ADR-0019](ADR-0019-issuer-pending-mint-commitment.md) | IssuerPosition `pending_mint_commitment` (two-phase mint linkage; I-03/I-07) | accepted |
| [ADR-0020](ADR-0020-spv-confirmation-fields.md) | BtcPayoutConfirmation SPV-completeness fields (tx index + inclusion headers) | accepted |
| [ADR-0021](ADR-0021-operator-position.md) | OperatorPosition struct + OPERATOR_REGISTER (bond ≥ 2× max claim) | accepted |

Reserved (placeholders to be filled during full M0, per PRD §14.2):
ADR-002 (BTC payout confirmation), ADR-003 (CR formula), ADR-004 (mint finalize CR re-check),
ADR-005 (zk toolchain), ADR-006 (BitVM3 lineage dispute).
