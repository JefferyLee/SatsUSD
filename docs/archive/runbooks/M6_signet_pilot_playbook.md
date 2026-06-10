# M6 Signet Pilot — Hands-on Playbook

This is the **operational playbook** for actually running SatUSD on signet. It
complements `M6_signet_deployment.md` (topology) and `M6_emergency_recovery.md`
(incident handling), and assumes the **regtest E2E**
(`crates/satusd-operator/tests/full_chain_devnet.rs`, `#[ignore]`) is green —
that test is the in-environment proof that deposit → mint → redeem → finalize →
reserve-spend all wire up against a real Bitcoin node.

Cross-references: `SECURITY.md` (trust model + gaps),
`integration/lineage_vectors/PROVENANCE.md` (fixture recapture),
`docs/decisions/INDEX.md` (ADRs).

---

## 1. Gap analysis: what's still missing before "real signet"

The **code path** for the full redemption is done and exercised on regtest.
What's missing is **signet-specific configuration, fixtures, and operational
glue**. Five concrete categories:

### A. Signet config (devops, **S**)

`devnet/` is a regtest stack. A `signet/` sibling is **not** in the repo. Need:
- `signet/conf/bitcoin.conf` — `signet=1`, signet peers / faucet endpoints,
  RPC port (suggest **38332** signet default), wallet config.
- `signet/conf/lnd.conf` — `bitcoin.signet=true`, point at signet bitcoind.
- `signet/conf/tapd.conf.template` — `network=signet`, point at lnd.
- `signet/signet-up.sh`, `signet-down.sh`, `env.sh` mirroring `devnet/`.
- Update `Makefile` with `signet-up` / `signet-down` targets.

Estimated effort: ~half-day (mostly config).

### B. Signet asset + recaptured lineage fixtures (fixture coupling, **M**)

The captured `*.hex` lineage vectors are bound to a specific devnet grouped
asset (family `0c58771b…`). On signet you'll mint a **new** grouped SatUSD
asset → **new family_id** → must rerun everything in
`integration/lineage_vectors/PROVENANCE.md`:

1. `signet-up`, fund signet wallet from a faucet.
2. Mint a grouped SatUSD asset on signet tapd.
3. `make recapture-vectors` (the 2 capture bins; for the other 4 transfer
   vectors run the Go oracle in `tools/tap-proof-oracle/`).
4. Update the `FAMILY` constants:
   - `crates/satusd-state/src/{claim,node}.rs`
   - `crates/satusd-operator/tests/full_chain_devnet.rs`
   - `crates/satusd-ta-proof/src/lib.rs::burn_to_sink_lineage_verifies` (the
     `c2945baf…` sink-key assertion)
5. `make vectors && make check` + `cargo test --test full_chain_devnet -- --ignored`
   (against signet bitcoind) — drift fails loudly.

Estimated: ~half-day, mostly waiting on signet confirmations.

### C. Live infrastructure currently stubbed/hardcoded (**M to L**)

These exist as functions/types but not as running services:

| Need | Currently | To do |
|---|---|---|
| **Live oracle signers** | `oracle_messages()` returns hardcoded pre-signed bytes (test fixture) | 5 oracle daemons signing real prices, exposed for fetch. **arm64 babyjubjub-rs sign panics** (M2 #29 note) — must sign via circomlibjs (Node) or a non-arm64 host. |
| **DA mirror service** | `satusd-da::LiveDABundle` defines the format; **no HTTP service** | Minimal axum endpoint: store bundles by `live_da_root`, serve `GET /v1/da/live/{root}`. The challenger expects to fetch. |
| **Challenger daemon** | `satusd-challenger::inspect` is a function | Daemon: poll the state-node's `/v1/state` for new claims, pull DA bundle, run `inspect` + `recompute_lineage` + `recompute_spv`, emit alerts. |
| **Keeper for reclaim_stale** | `node.reclaim_stale_claim` exists | Daemon: watch pending claims past `expiry_height` and call reclaim (RB-01). |
| **Deposit-confirmation watcher** | `mint_commit` takes `deposit_confirmations` as a **witness fact** | At minimum: a script that confirms a real deposit + presents to the issuer ops. (Full SPV proof of deposit is post-MVP.) |
| **Public read endpoint** | `GET /v1/state` exists | Expose behind reverse-proxy / Tailscale; add a thin `/v1/dashboard` aggregate. |

Estimated for the **minimum operational set** (DA mirror + challenger daemon +
keeper): ~2-3 days. The live oracle is the biggest item; for an alpha solo run
you can keep using pre-signed test oracle messages and treat the oracle role as
"manually rotated" until you stand up signers.

### D. Real keys + custody (**S–M, mostly process**)

The demo derives keys deterministically from labels (`reserve_demo`,
`full_chain_devnet`). For signet:

- **Committee (3-of-5)** — 5 real secp256k1 keys, distributed to 5 holders
  (HSM / hardware wallet / Yubikey-backed / separate machines). Use the same
  5 pubkeys for both governance (ECDSA over `claim_id`) and the BTC reserve
  spend (BIP143 sighash). Compute the **on-chain** P2WSH address from these
  pubkeys; fund it from the signet faucet.
- **Issuer multisig (2-of-3)** — same pattern, distinct from committee.
- **Oracle set (3-of-5 EdDSA-BabyJub)** — 5 babyjubjub keys per oracle node.
  The committed `oracle_set_hash` in the StateRoot pins this set.
- **User / operator wallets** — standard signet wallets per role.

Document the key set + the `reserve_committee_hash` + `oracle_set_hash` you
commit at genesis so the second-node operator can verify byte-identical state.

### E. Coordination + monitoring (**process**)

- A second state-node operator (third party). Share the genesis params:
  `(asset_family_id, oracle_set_hash, oracle_set_epoch, reserve_committee_hash,
  initial_l1_anchor, latest_oracle_price_e8)`. Both nodes must reach byte-
  identical `state_root_hash` after every transition.
- Postmortem channel + bug bounty channel live (per §14.13).
- A monitoring board (or just `tail -f` of state-node + challenger logs at
  start) for: pending claims, challenger alerts, committee approval lag,
  reserve UTXO health.
- A signet faucet plan: who drips signet BTC to issuer / operator / committee.

---

## 2. Phased rollout

Don't try to launch all 4 roles + third-party + dashboard at once. Three phases:

### Phase 1 — Solo signet alpha (you only, ~1 week of evening work)

Goal: **prove the same E2E test passes against a real signet bitcoind**, with
you holding all keys (committee, issuer, oracle, operator, user).

1. Build `signet/` config; bring up signet bitcoind+lnd+tapd. Verify `bitcoin-cli
   -signet getblockchaininfo` ok.
2. Fund signet wallet from a faucet (e.g. https://signet.bc-2.jp/,
   https://signetfaucet.com/, mutinynet-style faucets); plan ~0.05 signet BTC
   for the full reserve + HTLC + fees.
3. Mint a grouped SatUSD asset on signet tapd (`dtapcli mint` analog under the
   signet config).
4. Recapture lineage fixtures (§B above).
5. Adapt `full_chain_devnet.rs` → `full_chain_signet.rs` (copy, change
   `Network::Regtest` → `Network::Signet`, RPC port to signet, slow down the
   `generate_to_address` polling since you can't mine — wait for real
   confirmations). Run it.
6. **Acceptance**: the test prints the same `[1]..[9]` checklist on signet
   txids; the operator address receives `reimbursement_sats`.

What's still **manual** in Phase 1: oracle messages are still pre-signed (you
hold all 5 oracle keys); committee approvals are local; DA bundle is not
published to a mirror (the challenger isn't running).

### Phase 2 — Add the operational glue (the missing-service code, ~1 week)

1. **DA mirror service**: write a minimal axum service in a new bin (or in
   `satusd-state-node`) that stores `LiveDABundle` by `live_da_root` and serves
   `GET /v1/da/live/{root}`. Operator POSTs the bundle *before* `submit_claim`.
2. **Challenger daemon**: a binary that polls `/v1/state`, on a new claim
   fetches the bundle from the mirror, runs `inspect` + lineage/SPV recompute,
   logs/alerts on findings. Wire `veto_claim` for hard failures.
3. **Keeper daemon**: poll `pending_claims`, call `reclaim_stale_claim` past
   `expiry_height`.
4. **Public read** (`/v1/dashboard` JSON, or just publish `/v1/state` and a
   reserve view). Tailscale or a domain.
5. Re-run Phase 1 signet E2E with these services running: DA goes through the
   mirror, challenger watches, keeper would reclaim if you stop midway.

### Phase 3 — Third-party second node + an external operator (~2-4 weeks of coordination)

1. Find a third-party node operator (anyone you trust — another developer).
   Share the genesis config; they bring up their own state-node and connect to
   the same signet bitcoind (or their own, syncing the same chain).
2. After every transition, both nodes' `state_root_hash` must match
   byte-for-byte. **A mismatch is an immediate emergency-pause** (`M6_emergency_recovery.md`
   scenario 1).
3. Have someone other than you act as a user (lock + claim BTC HTLC) and/or
   operator (drive the BTC HTLC + submit_claim) for at least one redemption
   end-to-end.
4. **Acceptance**: a redemption initiated and finalized by parties who don't
   hold all 5 committee keys; node-A's StateRoot == node-B's; challenger emits
   no false-positive alerts; operator gets reimbursed.

---

## 3. Per-redemption operational checklist

For each redemption while running the pilot:

```
□ user constructs RedeemIntent (off-chain), agrees fees with operator
□ user locks SatUSD asset to derive_lock_script_key(intent) via tapd
□ user/operator submit REDEEM_FAST_LOCK to the state-node
□ committee/anyone verifies the lineage independently (sanity)
□ operator funds BTC HTLC for reimbursement_sats; informs user
□ user claims HTLC with preimage (on signet, real confirmations)
□ operator's observer waits 6 confirmations, builds BtcPayoutConfirmation
□ operator uploads Live DA bundle to ≥1 mirror
□ operator POSTs submit_claim to the state-node
□ challenger fetches DA, runs inspect → either ack or veto_claim
□ committee members each ECDSA-sign claim_id; ≥3 signatures collected
□ committee triggers finalize_claim (state-node + reserve_spend on signet)
□ verify operator received exactly reimbursement_sats on the signet block explorer
□ record in postmortem template if anything surprised you (even non-incidents)
```

---

## 4. Monitoring + alert response (minimum viable)

| Signal | Where to watch | First response |
|---|---|---|
| Two state-nodes diverge | compare `/v1/state` on both | `emergency_pause`, freeze, triage (`M6_emergency_recovery.md` §1) |
| Challenger alert | challenger daemon logs | acknowledge → triage; `veto_claim` if confirmed fraud |
| Pending claim past expiry, not finalized | keeper logs | keeper auto-reclaims; investigate why finalize stalled |
| Reserve UTXO stuck (signet mempool) | bitcoind getmempoolentry | pre-signed CPFP child (§3 of recovery runbook) |
| Faucet drip lapsed | committee logs | refund issuer reserve before next mint |
| Committee member offline | committee chat | run at 3-of-(N-1) if threshold still reachable; ROTATE_SHARD if not |

---

## 5. Failure mode catalog (what can actually go wrong)

Beyond the §13 adversarial cases (covered by code), watch for these
signet-specific operational failures:

1. **Signet reorgs are real** — signet does see occasional reorgs. SPV
   confirmations at 6 are designed for this, but if a reorg of depth > 6
   happens during finalize, the reserve-spend tx may need re-broadcast (RB-08
   on signet, partial coverage via `satusd-state::spv`).
2. **Faucet exhaustion / slow** — signet faucets are intermittent. Hold
   reserve buffer ≥ 2× max expected claim.
3. **tapd state desync** — tapd is young software; on signet you may hit
   bugs. Keep `dtapcli` snapshots before each capture; ExportProof failures
   are a known mode (capture bins have retry loops).
4. **Oracle quorum loss** — if 3 of 5 oracle signers go offline simultaneously,
   no claims can finalize. Have a procedure to publish updated `oracle_set_hash`
   (governance transition).
5. **Public endpoint DDoS / abuse** — if you expose `/v1/state` publicly,
   rate-limit. Read endpoints carry no auth.
6. **Key custody mishap** — committee key lost during pilot → ROTATE_SHARD
   (M6_emergency_recovery §4); never proceed without 3 confirmed key holders.

---

## 6. Exit criteria — when M6 is done

Per §14.13 acceptance + a few practical adds:

- ☐ A real signet redemption: deposit → mint → redeem → finalize → reimburse,
  with on-chain txids posted publicly.
- ☐ A second independent state-node consistently matches StateRoot byte-for-byte.
- ☐ A challenger (not the state-node operator) catches a *deliberately-induced*
  fraud (e.g. you submit a doctored lineage in a test redemption); alert flows;
  veto blocks finalize.
- ☐ At least one of: stuck-tx CPFP recovery / committee-rotation drill /
  emergency-pause-and-resume drill — exercised on signet, postmortem published.
- ☐ Public dashboard or read-only `/v1/state` exposed; users can verify supply
  / reserve / CR / claim list.
- ☐ Bug bounty channel open; one external reviewer engaged (even unpaid).

When all six are signed off, M6 is done and the project can plan for **M7
Optimistic Playground** (the on-chain dispute graph the dispute software model
already specifies — `satusd-dispute` + ADR-007).

---

## 7. Quick command reference (placeholder — fill once `signet/` lands)

```bash
# Bring up the signet stack (not yet implemented — Phase 1 #1):
make signet-up

# Refresh lineage fixtures (after a new signet asset mint):
make recapture-vectors
# … then update FAMILY constants per PROVENANCE.md, then:
make vectors && make check

# Run the signet E2E (after copying full_chain_devnet.rs → full_chain_signet.rs):
cargo test --test full_chain_signet -p satusd-operator -- --ignored --nocapture

# Start the state-node HTTP service:
cargo run -p satusd-state-node    # listens on :8088

# Start the challenger daemon (once written — Phase 2 #2):
cargo run -p satusd-challenger --bin challenger_daemon

# Start the DA mirror (once written — Phase 2 #1):
cargo run -p satusd-da --bin da_mirror

# A committee member signs an approval:
cargo run -p satusd-reserve --bin sign_approval -- --claim-id <hex> --key-file <path>
```

(Bins marked "once written" are the Phase-2 deliverables in §1.C — they are the
minimum operational additions to go from "regtest E2E passes" to "real signet
pilot can run".)
