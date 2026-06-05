# Workflows (SatUSD overrides)

See `~/Workplace/WORKFLOWS.md` for the cross-project patterns. This
file only documents SatUSD-specific overrides on top of them.

SatUSD is a BTC-reserve + Taproot Asset stablecoin protocol — a
software state machine (`StateRoot` + six SMTs) with Bitcoin/SPV,
TA-lineage, oracle, and burn verifiers, plus circom circuits and a
cross-language reference. There is no end-user UI; the project is
Rust crates + TypeScript reference + circom + regtest devnet. The
build is **not on mainnet**, conditionally approved for M0/M1 only
(see `docs/SatUSD_v5_1_PRD.md` header and `SECURITY.md` §Stage).

---

## `audit-and-tag` — SatUSD docs + ADR rule

The "docs vs code" agent specifically cross-checks:

- `docs/SatUSD_v5_1_PRD.md` — including the **errata backfill table**
  near the top (any drift PRD-vs-code becomes a new errata row, not a
  silent in-line PRD rewrite)
- `docs/decisions/INDEX.md` and the underlying ADRs (ADR-001..0023
  today). **ADR is the single source of authority** — don't relitigate
  ADR-accepted decisions in code review; instead, propose a superseding
  ADR.
- `docs/discovery/*_report.md` (G1/G2/G3 passed; G4/M1/M4/M5 reports
  pinned)
- `docs/discovery/adversarial_coverage.md` — every PRD §13 case maps
  to a code site or is marked deferred; new code that closes a `~partial`
  / `⏸ deferred` row should update this ledger
- `SECURITY.md` — the **trusted vs enforced vs deferred** table; any
  change that moves an item between columns must update it
- `ARCHITECTURE.md` — crate-layer DAG; new crates or layer crossings
  belong here

Cross-language invariant: Rust = TypeScript = circom. The "source audit"
agent flags drift between `crates/satusd-vectors`, `ts/src/`, and
`circuits/` — `make verify` must remain green (4481 assertions).

No human-facing tag sequence is established; tags so far follow the
milestone codes (M0..M8, G1..G4). The "tag spec" agent should propose
a name in that progression.

---

## `mvp-acceptance` — discovery gates + regtest E2E

SatUSD uses two acceptance shapes:

1. **Discovery gates (G1..G4)** — research/spec gates with explicit
   pass/fail criteria. Each closes with a `docs/discovery/G<n>_*_report.md`.
   Acceptance agent reads the report and confirms the gate's claims still
   match `crates/` + ADRs.
2. **Milestone milestones (M0..M8)** — code milestones whose proof is the
   **regtest E2E**: `crates/satusd-operator/tests/full_chain_devnet.rs`
   (`#[ignore]`, run with `cargo test --test full_chain_devnet -- --ignored`).
   This exercises deposit → mint → redeem → finalize → reserve-spend
   against a real bitcoind+lnd+tapd. Per `docs/runbooks/M6_signet_pilot_playbook.md`,
   this E2E is the in-environment proof that the milestone is real.

Each agent reports `done | partial | missing | unverifiable` per item;
verification is run by a fresh sub-agent (self-preferential bias guard).
For verifiers that need real Bitcoin behaviour, the agent must spin the
devnet (`make devnet-up`) — not stub it.

---

## `ship-feature` — SatUSD pipeline

There is no production deploy. The end of the loop is the local gate
plus, where relevant, the regtest E2E.

| Stage | Command |
|---|---|
| Format | `cargo fmt --all -- --check` |
| Lints | `cargo clippy --workspace --all-targets -- -D warnings` |
| Test | `cargo test --workspace` |
| Vectors | `make vectors` (regenerate `integration/vectors/vectors.json`) |
| Cross-lang | `make verify` (TS byte-matches Rust; first run needs `make ts-deps`) |
| Full gate | `make check` (does all of the above in order) |
| Regtest E2E (when reserve / BTC / tapd touched) | `make devnet-up && cargo test --test full_chain_devnet -- --ignored` |
| Circuit (when `circuits/` touched) | `circuits/check.sh` 3-way (Rust = TS = circom witness-calc) |
| Commit | conventional-commit style matching recent history (e.g. `mint: ...`, `M6 Phase 2: ...`, `docs: ...`) |
| Deploy | **none** — no mainnet, no signet yet; signet pilot is a separate runbook |

**Anti-pattern specific to SatUSD:**

- *Goal drift into PRD edits* — touching `SatUSD_v5_1_PRD.md` inline
  when an ADR is the right place. If your change reflects a real
  decision shift, write/extend an ADR; if it reflects "PRD says X,
  code does Y", add a row to the PRD errata table.
- *Skipping `make verify`* — Rust-only green is not the bar; the TS
  reference and (for circuit-touching changes) circom witness-calc
  must agree.

---

## `content-pass` — not applicable

SatUSD ships no LLM-authored user-facing content. The "content" here
is ADRs, PRD prose, runbooks, and discovery reports — authored by
Jeff with AI assist and reviewed by Jeff. The human-gate principle
already holds trivially.

## `screenshot-tour` — not applicable

No UI. SatUSD is Rust crates + a Node/TS reference + circom +
HTTP/JSON state-node API. Use the regtest E2E and `make check` as
the regression bar instead.
