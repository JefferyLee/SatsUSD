# FR-10 slash drill — equivocation detection, key extraction, manual slash

**Date**: 2026-06-12
**Authority**: PRD FR-10, M-B checklist; spec [05-dispute.md](../docs/spec/05-dispute.md) §2 §4
**Drill target**: a dedicated `oracled-drill` instance on the founder VPS — **not** the production live oracle (`943853cf…`). Drill scope: prove the slash pipeline end-to-end on signet infrastructure without disrupting production rails.

## What this drill demonstrates

The FR-10 acceptance criterion: *a planted deviation — a deliberate double attestation on the signet oracle — is detected and slashed end-to-end*. The drill executes the full path:

1. **Plant** a deliberate double attestation against a signet oracle instance.
2. **Detect** with `equivocation-verify` (any third party can run the same binary on the same bytes).
3. **Extract** the oracle's secret key as a self-certifying byproduct (EOTS — spec 03 §3.3).
4. **Slash**, Stage 1: rotate the oracle out, record the deviation per the scaffolding ledger.

## Drill oracle (now defunct)

| | |
|---|---|
| Instance | `oracled-drill` — separate from production `oracled` |
| Drill pubkey | `926daf74b19f09ee02446f9ef63a7f90f3e997705959ae3181acba90d77ef600` |
| Served on | `http://207.148.98.132:9591` (drill window only — service stopped, ufw rule removed) |
| Lifetime | ~6 seconds of liveness, ending in deliberate equivocation |
| Drill seed | published in [`fr10-drill/seed`](fr10-drill/seed) — **intentionally disclosed** as the post-drill state |

The drill pubkey was never bound to a rail manifest or a settlement. No production rail's `slash_conditions` reference it. The "slash" below is the disclosure + rotation flow per spec 05 §4 Stage 1, recorded for the scaffolding ledger.

## Evidence (self-certifying)

| Artifact | Path | Notes |
|---|---|---|
| Honest attestation | [`fr10-drill/att-1781276178.hex`](fr10-drill/att-1781276178.hex) | price = 60000, signed by drill oracle during its brief uptime |
| Conflicting attestation | [`fr10-drill/att-1781276178-conflict.hex`](fr10-drill/att-1781276178-conflict.hex) | price = 60001, same `event_id`, same predetermined nonce → leaks the key |
| Equivocation proof | [`fr10-drill/proof.json`](fr10-drill/proof.json) | output of `equivocation-verify` on the pair above |
| Drill metadata | [`fr10-drill/META.json`](fr10-drill/META.json) | reproducibility envelope |

Any third party, with only the two TLV hex files, reproduces the proof bit-exactly:

```sh
cargo build --release -p satusd-oracle --bin equivocation_verify
target/release/equivocation_verify \
    signet/fr10-drill/att-1781276178.hex \
    signet/fr10-drill/att-1781276178-conflict.hex
# → {"event_id":"SatUSD/BTCUSD/1781276178",
#    "oracle_pubkey":"926daf74…",
#    "digit_index":19,
#    "extracted_secret":"e3ddb8a8…"}
```

The verifier enforces `extracted_secret · G == oracle_pubkey` internally (`satusd_oracle::equivocation::verify_equivocation`, line 88–89). A non-zero exit means the pair was not a valid equivocation; this drill exits 0.

## Slash action (Stage 1)

Per spec 05 §4 Stage 1 the founder must, within one epoch:

1. **Execute the slash** per the affected rail's manifest `slash_conditions`.
   *Drill state*: no rail bound to the drill oracle's pubkey ⇒ no stake to seize. The slash in the rail-binding sense is vacuous for this drill instance — it exercises the path the manifest hook would invoke when a bound rail is the target.

2. **Publish the action with the evidence hash.** Recorded:

   ```
   evidence_hash = 868295119ae4c538d9e9852366ab9580f217aa2f2aed6a240d8fde0ded65658d
                 = sha256( att-1781276178.hex.bytes ‖ att-1781276178-conflict.hex.bytes )
   ```

   Recompute locally with:

   ```sh
   ( xxd -r -p < signet/fr10-drill/att-1781276178.hex
     xxd -r -p < signet/fr10-drill/att-1781276178-conflict.hex ) | shasum -a 256
   ```

3. **Rotate the oracle out.** Done: `systemctl stop oracled-drill`; ufw rule `9591/tcp` removed; the drill seed is now disclosed in this repo (the cryptographic state of the drill oracle is irreversibly compromised by the evidence itself, per spec 03 §3.3).

4. **Record in the rail's disclosure history** (FR-9 `dispute_record`). The drill oracle has no FR-9 disclosure record (no rail uses it). This file is the standalone deviation-ledger entry.

## Deviation-ledger row

```
Date          : 2026-06-12
Class         : oracle equivocation (spec 05 §2)
Detector      : founder (drill); reproducible by any third party
Evidence hash : 868295119ae4c538d9e9852366ab9580f217aa2f2aed6a240d8fde0ded65658d
Evidence      : signet/fr10-drill/att-1781276178.hex,
                signet/fr10-drill/att-1781276178-conflict.hex
Proof         : signet/fr10-drill/proof.json (self-certifying)
Action        : drill oracle pubkey 926daf74… rotated out; daemon stopped;
                port closed; seed disclosed
Bound rail    : none (drill instance never bound)
Status        : closed — slash flow Stage 1 executed end-to-end
```

## What this drill does *not* prove

- **Stage 2 enforcement** (script templates that pay the challenger automatically from the extracted key). Spec 05 §2 reserves this; it lands with the spec-04 Stage-2 ceremony.
- **Over-cap reimbursement evidence path** (spec 05 §3). That artifact format is pinned only once FR-9 disclosures land.
- **Founder-action latency under adversarial conditions.** The "founder MUST act within one epoch" requirement is procedural; this drill was self-initiated, not adversarial.

## Removing the scaffold

PRD §8 names this scaffold: *"Manual slash execution (FR-10 v0) — removal criterion: Stage-2 optimistic dispute flow live."* This drill closes the v0 acceptance; v0 is now demonstrated, not removed.
