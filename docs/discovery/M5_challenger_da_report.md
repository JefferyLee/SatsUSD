# M5 report: Live DA layer + challenger (detection mode)

- Date: 2026-05-26
- Scope: PRD §10 (DA layer), §5.D18 (challenger / dispute layering), §13.4
  (DA-01..06), §14.12 (M5 deliverables + acceptance), DL-23 (lineage), DL-22
  (BTC SPV). Relates to ADR-008 (live_da_root).
- Status: **verification core complete**; operational layers deferred (see end).

## What M5 needed vs. what was already built

The challenger's job (§5.D18) is to *independently* recompute a claim and alert on
mismatch — it never trusts the operator's `asset_proof_hash` or the state node's
acceptance. The two off-circuit verifiers it must re-run already existed from M3:

- TA lineage (DL-23) — `satusd-ta-proof::verify_lineage`.
- BTC SPV payout (DL-22) — `satusd-state::spv::verify_payout_confirmation`.

So M5's *novel* surface was (a) the Live DA bundle format + a real `live_da_root`,
and (b) the detection engine that orchestrates re-computation and emits alerts.

## Live DA bundle (`satusd-da`)

Wire format per §10.3: `SUSDDA01` header (magic | version | claim_id | state_epoch
| live_da_root | bundle_size) followed by ascending-id sections
`[id u8][size u32 BE][content]`, ids `0x01..0x0E`. Encode/decode round-trips; a
bad magic / truncation / size-mismatch is rejected.

**`live_da_root` definition (ADR-008).** The PRD gives two non-equivalent forms —
§10.1 `SHA256(canonical-encode(bundle))` and §10.3
`SHA256(concat(SHA256(section_i)))`. We adopt §10.3 (`section_i = id || size_be ||
content`): only the per-section root lets a challenger attribute a mismatch to a
*specific* missing/altered section, which DA-03 / DA-06 require. §10.1's phrasing
is superseded. Cross-language pinned (`live_da_root` vector: Rust = TS
`liveDaRootHex`).

**Real root wired into the claim.** `claim::live_da_root_for_claim` assembles the
bundle from the verified `ReserveClaimWitness` (sections 0x01 intents, 0x02 lock
records, 0x05 payout confirmations, 0x06 finalizes, 0x07 burn/lineage proofs, 0x08
oracle messages). `verify_reserve_claim_finalize` binds `claim.live_da_root ==
computed` (reject `LiveDaRootMismatch`) and records it in the new StateRoot;
`node::submit_claim` sets it. This replaces the prior `[0x0a; 32]` placeholder, so
the on-chain `live_da_root` now commits to a bundle the challenger can reconstruct.

## Challenger (`satusd-challenger`, detection mode)

`inspect(committed_live_da_root, bundle) -> Verdict { alerts }` plus
`recompute_lineage(proof_bytes)` and `recompute_spv(confirmation, …)`.

| Case | Scenario | Result |
|---|---|---|
| DA-01 | claim submitted, no DA bundle uploaded | `DaBundleMissing` |
| DA-02 | bundle does not hash to the committed `live_da_root` | `LiveDaRootMismatch` |
| DA-03 | bundle missing section 0x08 (oracle messages) | `MissingSection(0x08)` |
| DA-06 | bundle missing the lineage section | `MissingSection(0x07)` † |
| §14.12 | real burn-to-sink lineage | `recompute_lineage` = Ok |
| §14.12 | tampered lineage (fake) | `LineageInvalid` |
| DL-22 | invalid SPV confirmation | `SpvInvalid(..)` ‡ |

† PRD §10.3 reserves 0x03 for *lock-time* TA lineage proofs; in the claim bundle
the genesis→burn lineage the challenger re-runs travels in the universe-burn-proofs
section (0x07), so DA-06 ("missing lineage") maps there. The lock-time 0x03 section
is operator-supplied at the DA layer (deferred, below).

‡ The SPV positive path is covered by `satusd-state::spv` against real ground-PoW
regtest headers; the challenger test exercises the reject→alert forwarding.

## Acceptance (§14.12)

- Missing / mismatched DA detected — DA-01 / DA-02. ✓
- Fake lineage recomputed and alerted. ✓
- Invalid claim flagged — lineage (DL-23) and SPV (DL-22) re-run independently. ✓
  (The *full* software-verifier recompute — `verify_reserve_claim_finalize` over a
  DA-reconstructed witness — is deferred; see below.)

## Deferred (operational / out of the verification core)

Per the chosen M5 scope (verification core only):

- Multi-mirror fetch + selection (DA-04 / DA-05), Universe mirror service.
- 7-year archival DA storage + archival-fee accounting.
- Prometheus metrics + the reserve/supply/CR/oracle/claim dashboard.
- Audit export ZIP per `claim_id`.
- Full software-verifier recompute at the challenger (needs a bundle→witness
  decoder; the lineage + SPV re-runs already cover the cryptographic core).
- Lock-time TA lineage as a distinct 0x03 section in the claim bundle (the
  challenger re-runs the genesis→burn lineage carried in 0x07 today).

## Cross-language / test assurance

`make check` green: 161 tests, 4479 cross-language assertions across 28 domains.
`satusd-da` (6 tests) + `satusd-challenger` (8 tests) cover the bundle format,
`live_da_root` (3-way pinned), DA-01..06, and the lineage/SPV re-runs against the
real grouped-asset vector.
