# Signet ops — founder-run infrastructure runbook

Operational record for the PRD §8 scaffolding infrastructure.
No secrets live in this file: the VPS password is rotated and held
by Jeff; the oracle seed exists only on the server.

## VPS

| | |
|---|---|
| Host | 207.148.98.132 (Vultr) |
| OS | Ubuntu 22.04.5 LTS · 1 vCPU · 1 GB RAM (+1 GB swapfile) · 30 GB disk |
| Access | `ssh -i ~/.ssh/satusd_vps root@207.148.98.132` (ed25519 key on Jeff's machine; password auth remains enabled by choice, Vultr console as fallback) |
| Firewall | ufw: OpenSSH + 9590/tcp only |

## oracled (live since 2026-06-12)

| | |
|---|---|
| Service | systemd `oracled.service` — `Restart=always` |
| Binary | `/root/satusd-src/target/release/oracled` (built on the server from rsynced source) |
| Seed | `/root/satusd-oracle/seed` (hex, mode 600) — **the oracle's identity; never leaves the server, never entered a chat or repo.** Losing it = a new oracle pubkey (acceptable on signet; mainnet needs real key management) |
| Data | `/root/satusd-oracle/data` — `ann-<ts>.hex`, `att-<ts>.hex`, `latest.txt` |
| Retention | `/etc/cron.daily/oracled-prune` deletes `*.hex` older than 14 days (~3.5 GB steady state) |
| Public surface | `http://207.148.98.132:9590` — pubkey `943853cf7912f0f8515746e3c5db4aa97e9dc1a64648be925a647f10dcbd5019`, 1 s cadence, live 3-venue median price (since 2026-06-12; ticks with no fresh price are skipped, never back-filled) |

## Routine commands

```sh
SSH="ssh -i ~/.ssh/satusd_vps root@207.148.98.132"

$SSH systemctl status oracled            # health
$SSH journalctl -u oracled -n 50 --no-pager   # logs
$SSH systemctl restart oracled           # restart
curl -s http://207.148.98.132:9590/v0/latest  # external liveness
```

## Redeploy (after code changes)

```sh
rsync -az -e "ssh -i ~/.ssh/satusd_vps" --delete --exclude target --exclude .git \
  Cargo.toml Cargo.lock crates root@207.148.98.132:/root/satusd-src/
ssh -i ~/.ssh/satusd_vps root@207.148.98.132 \
  'source ~/.cargo/env && cd /root/satusd-src && \
   cargo build --release -p satusd-oracle --bin oracled && \
   systemctl restart oracled && sleep 2 && systemctl is-active oracled'
```

## Notes

- Changing the seed or any oracle parameter that alters announced
  events is a **new oracle** — announce it, don't blend histories.
- The instance is scaffolding (PRD §8 "founder-run single oracle");
  removal criterion: ≥ 1 independent oracle class live with market
  share. Anyone may mirror the data dir; clients verify signatures,
  never endpoints.
