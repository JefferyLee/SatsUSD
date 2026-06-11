# SatUSD Dispute Hooks

- **Spec**: 05
- **Version**: 0.1 — equivocation evidence normative and
  implemented; over-cap evidence structurally defined; Stage-1
  slash flow; Stage-2 reserved
- **Authority**: `docs/MISSION.md` v2 via ADR-0001; PRD FR-10/J6;
  staged enforcement per ADR-0002 / spec 04 §3

## 1. Model

A dispute hook is the path from **observable deviation** to
**enforced consequence**. v0 inherits the three-stage trust model
of spec 04 §3: Stage 1 enforces by publication + manual slash (a
PRD §8 ledger row); Stage 2 moves the same evidence formats behind
optimistic challenge windows; the formats are designed once, here,
so the stage transition swaps execution, not artifacts.

Two deviation classes are in scope for v0:

| class | detected by | artifact | consequence |
|---|---|---|---|
| oracle equivocation | anyone watching attestations | §2 | oracle key compromised + slash per manifest `slash_conditions` |
| over-cap reimbursement | anyone re-running the allotment plan | §3 | slash of the over-drawn rail's stake |

## 2. Equivocation evidence (normative)

Per spec 03 §3.3, an oracle that signs two different outcomes for
one event under its predetermined nonce leaks its secret key
(EOTS). **The evidence artifact is the pair of conflicting
`oracle_attestation` TLVs — nothing else.** No custom envelope, no
new encoding: both halves are already public, dlcspecs-format
bytes.

Validation (reference: `satusd-oracle::equivocation`):

1. Parse both TLVs strictly (`tlv::parse_attestation`; trailing
   bytes reject).
2. Same `event_id`, same oracle pubkey, same digit count — else
   not comparable.
3. Find the first digit index attested `0` in one and `1` in the
   other. Both signatures MUST verify under BIP-340 for their
   respective outcome messages and MUST share the committed nonce
   point (`R₁ = R₂`).
4. Extract `d = (s₁ − s₂)/(e₁ − e₂) mod n` and check `d·G`
   reproduces the oracle pubkey. The extracted key ships in the
   proof — **the artifact is self-certifying**: any verifier
   re-runs steps 1–4 from the two TLVs alone, with no trust in the
   challenger.

Consequences, by stage:

- **Stage 1**: challenger publishes the pair; the founder slashes
  the affected rail's oracle-linked stake per its manifest
  `slash_conditions` and rotates the oracle out. Failure to act on
  published evidence is itself public — the deviation-ledger
  discipline of spec 04 §3 applies.
- **Stage 2+**: the extracted key IS the enforcement: stake or
  settlement paths locked to the oracle key become spendable by
  anyone holding the proof. Equivocation needs no committee — the
  cryptography pays the challenger directly (design target;
  script templates with spec 04 Stage-2 ceremony).

## 3. Over-cap reimbursement evidence (structural)

The claim "rail X drew more than its tranche in epoch E" is a pure
recomputation over public data:

```
evidence = (epoch E,
            allotment input data        — per FR-9 disclosures,
            observed reimbursement set  — S3 artifacts, spec 02 §4)
check    = recompute AllotmentPlan(input)         (FR-6, satusd-allot)
           assert plan.input_hash matches the published plan
           sum reimbursements(rail X, E) > plan.tranche(rail X)
```

The byte-exact artifact format is pinned when FR-9's disclosure
format lands (the reimbursement set's canonical encoding is the
missing half). Until then the check is reproducible from published
JSON + chain data — weaker packaging, same math. Stage 1
consequence: founder slashes per ledger; the published plan-hash
mismatch or over-draw sum is the deviation record.

## 4. Slash flow, Stage 1

1. Challenger obtains evidence (§2 pair or §3 recomputation).
2. Publishes it anywhere durable + notifies the founder (transport
   deliberately unstandardized, as ADR-0002 treats quote
   transport).
3. Founder MUST act within one epoch: execute the slash per the
   rail's manifest `slash_conditions`, publish the action with the
   evidence hash, and record it in the rail's disclosure history
   (FR-9 field `dispute_record`).
4. Inaction or disagreement is public: the evidence is
   self-certifying, so "the founder ignored a valid proof" is
   itself checkable — and is exactly the failure mode the Stage-2
   transition removes.

FR-10 acceptance (PRD): a planted deviation — a deliberate double
attestation on the signet oracle — is detected and slashed
end-to-end. The detection and validation halves are implemented
and tested (`equivocation::tests`); the drill itself is a signet
operation.

## 5. Open

- Challenger reward economics (who pays, how much — protocol fees
  per the cost principle; sizing TBD with spec 04 §4).
- Stage-2 script templates binding §2 proofs to spendable paths.
- Bad-proof (invalid S3 artifact) evidence — needs FR-7's
  verification library surface to freeze first.
- k-of-n equivocation semantics (slash one signer vs. the set).
