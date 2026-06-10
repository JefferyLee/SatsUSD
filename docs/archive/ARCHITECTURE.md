# SatUSD — Architecture

A map for navigating the codebase: the crate layering, what each crate does, and
the end-to-end redemption flow that ties them together. Spec: `docs/SatUSD_v5_1_PRD.md`
(+ its errata table); decisions: `docs/decisions/INDEX.md`; trust model:
`SECURITY.md`.

SatUSD is a BTC-reserve + Taproot Asset payment stablecoin. The core protocol is a
**state machine** (the `StateRoot` + six SMTs) advanced by typed transitions, with
every external "fact" (BTC SPV, TA lineage, oracle price, burn) replaced by an
**independently re-runnable verifier**. The reserve is committee-gated today
(MVP) and optimistic/BitVM3 later.

## Crate layers (dependency DAG, leaf → root)

```
 L0  satusd-types ............ canonical encoding (§18), domain registry, structs, derived hashes
 L1  satusd-crypto (→types) .. Poseidon/SMT (BN254), NUMS/TapTweak, ECDSA, EdDSA-BabyJub, state_root_hash
     satusd-da     (→types) .. Live DA bundle format (§10.3) + live_da_root
 L2  satusd-ta-proof (→crypto) .... tapd proof.File verifier: TA lineage (DL-23), burn (D16)
     satusd-lock    (→crypto,types) . Bitcoin-layer lock anchor (§5.D3) + lock_script_key
     satusd-reserve (→types,crypto) . ReserveBackend (§5.D9): Mock/Multisig + committee + Optimistic trait
 L3  satusd-state  (→types,crypto,ta-proof,da,reserve) .. THE state machine: mint/redeem/claim/
     |                                                     registry/oracle/spv + StateNode engine
     satusd-dispute (→types,crypto,ta-proof,reserve) ..... BitVM3 dispute resolution (§5.D18) + PlaygroundReserve
 L4  satusd-state-node (→state) .... axum HTTP API (§7.3): /v1/state, /v1/transition/{simulate,submit}
     satusd-challenger (→state,da,ta-proof) .. detection: DA inspect + recompute lineage/SPV (M5)
 L5  satusd-operator (→state, state-node[dev]) .. quote / BTC HTLC / observer / HTTP submit (§14.6)
     satusd-tapd-client (→lock,ta-proof,crypto) .. tonic gRPC to tapd; wallet + capture bins
 tooling
     satusd-vectors (→types,crypto,da) .. generates the cross-language test vectors
```

Off-tree references: `ts/` (TypeScript reference — must byte-match `satusd-vectors`
output via `make verify`), `circuits/` (circom + `check.sh` 3-way + `prove.sh`/
`setup.sh`), `integration/` (vectors.json + real tapd `lineage_vectors/`),
`devnet/` (native regtest bitcoind+lnd+tapd), `tools/tap-proof-oracle/` (Go, fixture
capture), `docs/{decisions,discovery,runbooks}`.

## What each crate owns

| Crate | Owns |
|---|---|
| `satusd-types` | §18 canonical encode/decode, domain separators, all frozen structs (StateRoot, RedeemIntent, ReserveClaim, …), derived hashes (claim_id, lock_tweak, …) |
| `satusd-crypto` | Poseidon + SMT (ADR-0013/0015), NUMS/TapTweak (ADR-0014), ECDSA, EdDSA-BabyJub, `state_root_hash` (Poseidon-of-fields, ADR-010) |
| `satusd-da` | Live DA bundle (`SUSDDA01` header + sections) + `live_da_root` (§10.3, ADR-008) |
| `satusd-ta-proof` | tapd `proof.File` parser + lineage verifier (DL-23) + burn binding (D16) |
| `satusd-lock` | Bitcoin-layer P2TR lock anchor (NUMS + finalize/refund leaves) + `derive_lock_script_key` |
| `satusd-reserve` | `ReserveBackend` (§5.D9): MockReserve, MultisigReserve (3-of-5 + pause/veto/rotate), `OptimisticEnforcementBackend` trait |
| `satusd-state` | the transition verifiers (mint/redeem/claim/registry/oracle/spv) + `StateNode` (executes + commits + self-checks; holds the reserve backend) |
| `satusd-state-node` | axum HTTP wrapper around `StateNode` + the JSON DTOs |
| `satusd-challenger` | M5 detection: `inspect` a claim's DA + `recompute_lineage`/`recompute_spv` |
| `satusd-dispute` | M8 dispute resolution (lineage/lock-binding) + `PlaygroundReserve` optimistic model |
| `satusd-operator` | quote engine, BTC HTLC builder, confirmed-payout observer, HTTP claim submitter |
| `satusd-tapd-client` | gRPC client to tapd; `wallet` + `capture_*` fixture bins + proof assembly |
| `satusd-vectors` | regenerates `integration/vectors/vectors.json` (the cross-language ground truth) |

## End-to-end: a fast redemption (the core flow)

```
user + operator
   │  agree RedeemIntent (amount, payment_hash, fees)
   ▼
[1] user locks SatUSD → lock anchor          satusd-lock (derive_lock_script_key)
   │   on-chain via tapd                       satusd-tapd-client
   ▼
[2] REDEEM_FAST_LOCK                          satusd-state::redeem
   │   verify TA lineage (DL-23) +             satusd-ta-proof
   │   intent↔lock binding (§5.D15)            → StateNode commits (lock_record_root)
   ▼
[3] operator funds BTC HTLC; user claims      satusd-operator::htlc
   │   with preimage; operator observes        satusd-operator::observer
   │   the buried claim → BtcPayoutConfirmation  → satusd-state::spv (DL-22, R-15)
   ▼
[4] operator burns SatUSD → protocol sink;    satusd-state::claim (D16) / satusd-ta-proof
   │   uploads Live DA bundle;                 satusd-da
   │   POST /v1/transition/submit              satusd-operator::submit → satusd-state-node
   ▼
[5] submit_claim (REDEEM_FAST_FINALIZE)       satusd-state::{claim,oracle,node}
   │   oracle quorum (§5.D7) + SPV + burn +    satusd-state::oracle / spv
   │   lineage all re-verified; reserve the    satusd-reserve (custody view)
   │   reimbursement (two-phase, ADR-0022)
   ▼
[6] challenger pulls DA, recomputes           satusd-challenger (detection)
   │   lineage/SPV; alert/veto on mismatch     satusd-dispute (M8 on-chain Disprove)
   ▼
[7] committee approves (3-of-5 ECDSA) →        satusd-reserve::MultisigReserve
       finalize_claim pays out                 satusd-state::registry (consensus gate)
       (M7/M8: assert → challenge window →      satusd-dispute::PlaygroundReserve
        disprove | withdraw)
```

Cross-cutting: the **circuits** (`circuits/`) prove the Poseidon-friendly pieces
(cr/tier, SMT folds, oracle EdDSA, the `m7_transition` state commitment), and
**`satusd-vectors` + `ts/`** keep Rust = TypeScript = circom byte-aligned. What is
proven vs trusted vs deferred is in `SECURITY.md`.

## Where to start reading

- **Protocol logic**: `satusd-state/src/{node,claim,redeem,mint,registry,oracle}.rs`.
- **The state commitment + crypto**: `satusd-crypto/src/{state,poseidon,smt}.rs`.
- **External-fact verifiers**: `satusd-ta-proof` (lineage/burn), `satusd-state/src/spv.rs`.
- **HTTP surface**: `satusd-state-node/src/{lib,dto}.rs`.
- **Circuits**: `circuits/m7_transition.circom` (+ `check.sh`).
- **Build/verify**: `make check` (fmt/clippy/test/vectors/verify); `circuits/check.sh`
  (3-way); `make devnet-up` then `make recapture-vectors` for the live fixtures.
