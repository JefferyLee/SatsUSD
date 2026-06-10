# M6 Signet Deployment Runbook (Multisig Pilot)

- PRD reference: §14.13 (M6 deliverables), §11.2 (MultisigReserve), §4 (dual state
  node), §2.3 KPIs. Code: `satusd-reserve` (`MultisigReserve`), `satusd-state`
  (`StateNode::use_multisig_reserve`), `satusd-state-node` (HTTP), `satusd-operator`,
  `satusd-challenger`, `devnet/` (the regtest stack this adapts).
- Status: deployment recipe for the signet pilot. The code path
  (committee-gated finalize + pause/veto/rotate) is implemented and unit-tested;
  this runbook is the operational procedure to run it on signet with real BTC and
  ≥ 1 third-party node. It is **not** executed in CI (no signet access here).
- **For step-by-step execution + the missing-services gap analysis + phased
  rollout, see the companion `M6_signet_pilot_playbook.md`.**

## Topology

```
            ┌── state node A (project) ──┐        ┌── challenger (project) ──┐
 signet ───►│  satusd-state-node :8088   │◄──DA──►│  satusd-challenger       │
 bitcoind   └────────────┬───────────────┘        └──────────────────────────┘
 + lnd/tapd              │ byte-identical StateRoot
            ┌── state node B (3rd party) ┘
            └── operator(s): quote / HTLC / observer / submit
                          │
            reserve BTC: 3-of-5 P2WSH/P2TR multisig (committee, distinct from issuers)
```

Hard requirement (§4): **two** independent state-node instances (project + ≥ 1
third party) that reach byte-identical `StateRoot` on every transition. A
divergence halts finalize (runbook scenario 1).

## Adapting the stack from `devnet/`

The regtest `devnet/` stack (bitcoind + lnd + tapd, ADR-0017) becomes signet by
configuration only:

- `bitcoind`: `signet=1` (+ optional `signetchallenge=` for a custom signet);
  drop `regtest=1`. Fund from a signet faucet, not `generatetoaddress`.
- `lnd` / `tapd`: `--bitcoin.signet` (replace `--bitcoin.regtest`); point at the
  signet bitcoind. Re-mint the SatUSD asset family on signet and **re-capture the
  lineage vectors** (`integration/lineage_vectors/*.hex`) + update the family
  constants (the real-vector ↔ test coupling note in `project_key_decisions`).
- Confirmation depths use real signet timing; keep `BTC_CLAIM_CONFIRMATION_DEPTH`.

## Bring-up sequence

1. **Reserve multisig.** The 5 committee members (keys distinct from issuers)
   generate keys and form the 3-of-5 P2WSH/P2TR address. Fund the reserve UTXO from
   signet. Pre-sign the CPFP fee-bump child (runbook scenario 3).
2. **State node A** (`satusd-state-node`): `genesis(...)` then
   `use_multisig_reserve(Committee { threshold: 3, pubkeys })` **before** any
   deposit, so the custody backend seeds at 0 and the committed
   `reserve_committee_hash` matches. Serve `/v1/transition/{simulate,submit}`.
3. **State node B** (third party): same genesis + committee; the two `StateRoot`
   hashes must match after every transition (compare `/v1/state`).
4. **Operator(s)**: quote → fund BTC HTLC → observe confirmed payout → POST the
   batched `submit_claim` (`satusd_operator::submit_transition`).
5. **Challenger**: subscribe to submitted claims, pull the Live DA bundle, run
   `inspect` + `recompute_lineage` + `recompute_spv`; file `veto_claim` on a real
   discrepancy (node API `veto_claim` → `MultisigReserve::veto`).
6. **Committee**: review each claim, sign approvals over `claim_id`
   (`satusd_reserve::sign_claim_approval`); node `finalize_claim` pays only at
   3-of-5 and when not paused/vetoed.

## End-to-end acceptance (§14.13)

Drive one real signet redemption through: **deposit → mint → redeem (lock) →
submit_claim → committee approves → finalize → reserve spend reimburses the
operator**. Confirm node A and node B agree on every `StateRoot`, and the reserve
spend pays exactly `reimbursement_sats`.

## Operations

- **Emergency pause / resume / veto / committee rotation**: node API
  `emergency_pause`, `resume_reserve`, `veto_claim`, `rotate_reserve_committee`
  (ROTATE_SHARD — recommits `reserve_committee_hash` and drops stale approvals).
  Failure handling: `docs/runbooks/M6_emergency_recovery.md` (5 scenarios).
- **Dashboard**: publish supply / reserve / CR / oracle / claim / tier from
  `/v1/state` + `reserve_view`. (A read-only aggregator; deferred — not in the
  verification core.)
- **Bug bounty + postmortems**: per §14.13 / the recovery runbook template.

## Deferred (beyond the M6 code core)

Live signet bring-up itself, the third-party node operator agreement, the public
dashboard service, Prometheus metrics, and 7-year archival storage are pilot
operations, not code in this repo.
