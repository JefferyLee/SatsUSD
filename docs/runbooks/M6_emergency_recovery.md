# M6 Emergency Recovery Runbook (MultisigReserve, Signet Pilot)

- PRD reference: §11.2 (MultisigReserve), §5.D9 (reserve backend), §5.D11
  (FreezeReason), §14.13 (M6 deliverables). Code: `satusd-reserve`
  (`MultisigReserve`), `satusd-state::registry` (FINALIZE_CLAIM, ADR-0023).
- Audience: the 3-of-5 reserve committee + the operator(s) running the signet
  pilot. The committee key set is distinct from the issuer set (§4).
- Status: pilot runbook. The MultisigReserve approval state machine is implemented
  + unit-tested; the live signet reserve-spend transaction, the second
  (third-party) state node, and the public dashboard are pilot-deployment work.

## Standing facts

- Reserve BTC is held in a 3-of-5 P2WSH/P2TR multisig (committee keys).
- `submit_claim` (REDEEM_FAST_FINALIZE) burns SatUSD supply and **reserves** the
  reimbursement; it does **not** move reserve BTC. `MultisigReserve::submit_claim`
  records the pending claim (`reserved_sats += amount`).
- A claim is paid only at `finalize_claim`, which requires **≥ 3 distinct,
  authorized, valid committee approvals over `claim_id`** (ECDSA; the committed
  `reserve_committee_hash` in `StateRoot` pins the member set — ADR-0023). The
  approval rule is `satusd_reserve::count_approvals`, identical to
  `registry::verify_finalize_claim`.
- A challenger files `veto_package(claim_id)`; the committee discusses before
  finalizing (challenger detection is `satusd-challenger`, M5).
- `emergency_pause` blocks **all** finalize until `resume()`.

## Scenario 1 — State-node bug suspected

1. Any committee member calls `emergency_pause(StateNodeBug)` — every
   `finalize_claim` now returns `Paused`.
2. Post a public notice (status page + pilot channel) with the suspected scope.
3. Reproduce against the second (independent) state node: the two `StateRoot`s
   must byte-match (§4 dual-instance requirement). A divergence localizes the bug.
4. Fix + re-verify on both nodes against the same transitions. Only then `resume()`.
5. File a postmortem (template below) within 14 days.

## Scenario 2 — User/operator reports funds at risk

1. Committee votes `emergency_pause(FundsAtRisk)`.
2. Triage: re-run the challenger (`satusd-challenger::inspect` + `recompute_lineage`
   + `recompute_spv`) over the implicated claim's DA bundle. A real discrepancy →
   keep paused + `veto_package` the claim; a false alarm → `resume()`.
3. Notice + postmortem as above.

## Scenario 3 — Reserve UTXO stuck (fee too low)

1. The reserve-spend tx is stuck in the mempool. Do **not** rebuild blindly.
2. Broadcast the **pre-signed CPFP** child (committee pre-signs a fee-bump child at
   spend time; the anchor output exists for this) to bump the package fee.
3. If no pre-signed CPFP is available, the committee co-signs a replacement (RBF)
   spend of the same reserve input — never a second spend of a different input for
   the same `claim_id` (would double-pay).
4. Confirm the operator received exactly `reimbursement_sats`; record txids.

## Scenario 4 — Committee member key lost

1. The member declares the key lost; the committee continues at 3-of-(4 remaining)
   as long as threshold is still reachable. If not, **pause**.
2. Run the `ROTATE_SHARD` stub: stand up a replacement member key, form the new
   committee set, and commit the new `reserve_committee_hash` to `StateRoot` (the
   on-chain pin must change atomically — old approvals over `claim_id` from the
   rotated-out member no longer count).
3. Public notice naming the rotation (not the key material) + postmortem.

## Scenario 5 — Any incident → postmortem

Every pause/veto/rotation/stuck-tx event is written up using the template below and
published within **14 days**. No silent recovery.

---

## Postmortem template

```
# Postmortem: <short title>

- Date of incident:
- Date published (≤ 14 days after):
- Severity: [funds-at-risk | availability | process]
- Authors:

## Summary
<2–3 sentences: what happened, impact, current status.>

## Timeline (UTC)
- HH:MM  <event — detection, pause, vote, fix, resume>
- ...

## Impact
- Claims affected (claim_id, operator, sats):
- Reserve sats at risk / moved:
- User-visible effect:

## Root cause
<the actual cause, not the trigger.>

## Detection
<how it was caught: challenger alert / committee / operator report / monitoring.>

## Resolution
<what restored safe operation, incl. txids for any reserve spend / CPFP / RBF.>

## What went well / what didn't

## Action items
- [ ] <owner> — <fix / guardrail / runbook update> — due <date>
```
