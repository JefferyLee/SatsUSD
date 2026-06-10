# Archive — the v5.1 era (superseded)

Everything in this directory describes the project **before the
2026-06-09 restart** and is retained for historical reference only.
None of it is authoritative.

| Content | What it was |
|---|---|
| `SatUSD_v5_1_PRD.md` | The v5.1/v5.2 product requirements document — committee-gated reserve, single-oracle quorum, M0–M8 milestone plan |
| `decisions/` | ADR-001 .. ADR-0023 — the superseded decision series |
| `discovery/` | G1–G4 gate reports and M-milestone discovery reports |
| `runbooks/` | M6 signet pilot runbooks for the old architecture |
| `SECURITY.md` | Trust model of the old architecture (trusted/enforced/deferred table) |
| `ARCHITECTURE.md` | Crate-layer map of the old architecture |

## Why the restart

The project explored several directions; some failed technically,
some drifted from the original intent. The seed vision was rewritten
as `docs/MISSION.md` (v2) — price-signal migration, oracle
marketization, self-referencing as the definition of success, the
bridge that retires — and the project was rebuilt from it.

The restart decision, including the code salvage map (what survives
from this era and what is rewritten), is recorded in
`docs/decisions/ADR-0001-restart-from-mission-v2.md`.

These documents remain valuable: they record what was tried, what
worked, what didn't, and why. Per the mission — "what we learn will
be shared."
