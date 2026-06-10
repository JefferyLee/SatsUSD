# Workflows (SatUSD overrides)

See `~/Workplace/WORKFLOWS.md` for the cross-project patterns. This
file only documents SatUSD-specific overrides on top of them.

SatUSD is a bitcoin-collateralized, dollar-denominated Taproot Asset
on Bitcoin L1, convertible through an open standard of competing
**rails** (spec 02). The project was **rebuilt 2026-06-09 from
MISSION v2** (ADR-0001); the v5.1-era architecture is archived.

Document hierarchy (conflicts resolve upward):

```
MISSION.md / MISSION.zh.md   why        — supreme authority
PRD.md                       what       — scope, FRs, milestones,
                                          scaffolding ledger
docs/spec/*                  how        — normative protocol detail
docs/decisions/ADR-0001+     decisions  — the active ADR series
docs/archive/*               history    — FROZEN, never authority
```

---

## `audit-and-tag` — SatUSD docs + ADR rules

The "docs vs code" agent cross-checks, in this order of authority:

- `docs/MISSION.md` + `docs/MISSION.zh.md` — **EN/zh parity is an
  invariant**: any change to one edition without the other is a
  finding.
- `docs/PRD.md` — the FR table (§4) against code; the **scaffolding
  ledger (§8)** against reality: every founder-dependent role in the
  running system MUST have a ledger row with a removal criterion.
  An un-ledgered founder dependency is a finding of the highest
  severity.
- `docs/spec/*` — spec vs implementation drift. Code implementing a
  spec section SHOULD cite it; spec changes that reflect a decision
  shift need an ADR first.
- `docs/decisions/` — **the active series starts at ADR-0001
  (2026-06-09)**. Don't relitigate accepted ADRs in review; propose
  a superseding ADR.
- `docs/archive/` — frozen. Editing an archived file, or citing one
  as authority for current behaviour, is a finding. (Reading them
  for history is fine — that's what they're for.)
- `docs/proposals/` — live external drafts. Known pending revision:
  0001's oracle assumptions predate the rail/oracle-class model.

Cross-language invariant: **Rust = TypeScript** via
`make vectors && make verify`. The former three-way check including
circom is **suspended** — `circuits/` is on hold per the ADR-0001
salvage map; re-add the third leg if/when circuits are re-judged.

Tag naming: the old M0..M8 / G1..G4 progression is retired with the
archive. New tags follow the PRD milestone codes **M-A..M-E**
(e.g. `M-A-regtest`), plus spec versions for standard-only releases.

---

## `mvp-acceptance` — PRD-driven, metric-gated

Acceptance now has exactly two sources, both in `docs/PRD.md`:

1. **FR-1..FR-10 (§4)** — each row carries its own acceptance
   check, deliberately machine-decidable (e.g. FR-6: "a third party
   re-runs the allotment script on public data and reproduces the
   plan byte-for-byte"). The acceptance agent executes the check as
   written, not a paraphrase of it.
2. **Milestone checklists M-A..M-E (§7)** — metric-gated, no dates.
   A milestone is closed when every box checks; partial credit does
   not exist.

The regtest E2E bar is the **J3/J4 journeys** (Rail-0 RFQ swap,
Rail-1 DLC) once implemented. The archived-era
`full_chain_devnet.rs` still builds (old crates keep building per
ADR-0001) but exercises the superseded committee architecture — it
is **not** an acceptance artifact for the new world; don't cite it
as one.

Each agent reports `done | partial | missing | unverifiable`;
verification runs in a fresh sub-agent (self-preferential bias
guard). Verifiers needing real Bitcoin behaviour spin the devnet
(`make devnet-up`) — never stub it.

---

## `ship-feature` — SatUSD pipeline

| Stage | Command |
|---|---|
| Format | `cargo fmt --all -- --check` |
| Lints | `cargo clippy --workspace --all-targets -- -D warnings` |
| Test | `cargo test --workspace` |
| Vectors | `make vectors` |
| Cross-lang | `make verify` (Rust = TS; first run needs `make ts-deps`) |
| Full gate | `make check` |
| Regtest E2E (when rail / reserve / tapd code touched) | `make devnet-up` + the J3/J4 journey tests |
| Circuit check | **suspended** (circuits on hold, ADR-0001) |
| Commit | conventional style matching the new history (`spec:`, `mission:`, `restart:`, `rail0:`, `docs:`) |
| Deploy | **none** — regtest until M-A, signet at M-B, mainnet at M-C |

`make check` MUST stay green throughout the rebuild: old crates keep
building until their replacements land — that suite guards the kept
primitives (`satusd-crypto`, `satusd-ta-proof`, vectors).

**Anti-patterns specific to SatUSD:**

- *Un-ledgered scaffolding* — any change that adds a
  founder-dependent role (a server only Jeff runs, a key only Jeff
  holds, a step only Jeff executes) MUST add a PRD §8 ledger row
  with a removal criterion **in the same commit**. No silent
  centralization.
- *Skipping the mission criterion* — any change introducing or
  deepening an external dependency (oracle, data source, service)
  must answer, in the PR/commit description: *does this move us
  closer to or further from the state where the external dependency
  can be removed?* (MISSION, self-referencing section.)
- *Editing PRD/spec to smuggle a decision* — decision changes go
  through an ADR first; PRD/spec then update to match. (Replaces
  the old "PRD errata table" rule, which is archived with v5.1.)
- *Touching one MISSION edition without the other* — EN and zh ship
  together or not at all.
- *Skipping `make verify`* — Rust-only green is not the bar.

---

## `content-pass` — public-facing documents

The project now ships public prose: MISSION (EN/zh), PRD, and
`docs/proposals/*` destined for dlcspecs / Delving Bitcoin /
lightning-dev. Overrides on the human gate:

- Jeff is the gate for anything public; the model never pushes
  mission-level or proposal text without explicit approval (drafts
  in-repo are fine).
- External proposals are **bilingual EN/中文** and carry an
  AI-assist disclosure in the cover post (established pattern:
  ideas/construction attributed to the author, prose polished with
  Claude Code, "errors are still mine").
- Factual claims in public docs must survive screenshot scrutiny —
  every number and competitor characterization gets verified before
  push (the "halved in fifteen years" lesson).

## `screenshot-tour` — not applicable yet

No UI. Becomes applicable when the reference wallet (PRD open
question 1) lands; until then the regression bar is `make check` +
the J3/J4 journeys.
