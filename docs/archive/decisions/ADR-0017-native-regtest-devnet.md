# ADR-0017: Native-binary regtest devnet (interim)

- **Date**: 2026-05-24
- **Status**: accepted
- **PRD reference**: §9.1, §14.4 (M1), T-M1W1-01
- **Author**: Jeffery (with implementation from AI coding agent)

## Context

G1 (TA lock plan A/B) and M1 need a working regtest stack of bitcoind + lnd +
tapd. The PRD (§9.1, T-M1W1-01) specifies a docker-compose devnet
(`make devnet-up`, bitcoind + lnd ×2 + tapd ×2 + universe + …). On this machine
the Docker daemon was not running (Docker Desktop installed but stopped, old
20.10 client), and we wanted the fastest path to a usable G1 environment.

## Options Considered

1. **Native prebuilt binaries as host processes** (chosen). Download lnd + tapd
   darwin-arm64 release binaries, run bitcoind (host) + lnd + tapd via scripts.
   - Pros: no Docker dependency; fast; easy to drive with lncli/tapcli; the stale
     host lnd 0.15.99 is irrelevant (devnet uses pinned binaries in devnet/bin).
   - Cons: single-node; diverges from the PRD docker-compose; macOS/arch-specific
     fetch script; not the multi-node M1 topology yet.
2. Docker-compose (PRD-aligned). Deferred: requires starting Docker Desktop and
   is heavier; revisit for the multi-node M1 devnet and for CI.

## Decision

Use a **native-binary regtest devnet** as the interim G1/early-M1 environment:

- Pinned: lnd **v0.20.1-beta**, tapd **v0.7.2** (compatible: tapd 0.7.2's go.mod
  targets the lnd 0.20.0-beta line; ≥ PRD floor lnd 0.17 / tapd 0.4), bitcoind
  from PATH (v31).
- `devnet/`: committed configs + scripts (`fetch-binaries.sh`, `devnet-up.sh`,
  `devnet-down.sh`, `env.sh`); `bin/`, `data/`, `logs/` gitignored.
- `make devnet-up` / `make devnet-down`. lnd uses `--noseedbackup` (regtest only).

Verified end to end: bitcoind 101+ blocks, lnd synced + funded, tapd connected,
and a test asset minted/confirmed (`decimal_display = 2`, matching D9).

## Consequences

- Positive: G1 is unblocked now without Docker.
- Trade-off: not yet the PRD's docker-compose multi-node devnet. A second
  lnd/tapd pair (for transfer/redemption flows) and a docker-compose option for
  CI remain follow-ups for M1; this ADR is the interim, to be superseded then.
- Platform: `fetch-binaries.sh` is darwin-arm64; other platforms need adjusted
  asset names.

## References

- `devnet/` (scripts, configs, README)
- lnd v0.20.1-beta, taproot-assets v0.7.2 GitHub releases
