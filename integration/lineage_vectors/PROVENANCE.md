# Lineage fixture provenance & recapture

These `*.hex` files are **real tapd `proof.File` bytes** captured from a live devnet
(bitcoind + lnd + tapd v0.7.2). They are the ground truth the Taproot Asset lineage
verifier (`satusd-ta-proof`, DL-23), the §5.D16 burn binding, the §5.D15 lock
binding, the M5 challenger, and the M8 dispute resolver are all tested against.

Because they come from a *specific* devnet asset, several constants in the code are
**pinned to that asset** (its family id, sink/lock script keys, and a canonical
`RedeemIntent`). Rebuilding the devnet mints a *new* asset → these fixtures and the
pinned constants must be regenerated together. Drift is designed to **fail loudly**
(the captured head script key stops matching the derived one), never silently.

## The fixtures

| File | What it is | Source | Consumers |
|---|---|---|---|
| `genesis.hex` | single-asset genesis proof | manual (ExportProof) | `satusd-ta-proof` tests |
| `transfer.hex` | single-asset transfer/split | manual | `satusd-ta-proof` tests |
| `grouped_transfer.hex` | **grouped** asset transfer (group key) | manual | `satusd-ta-proof`, `satusd-state::redeem` |
| `burn_transfer.hex` | grouped transfer toward burn | manual | `satusd-ta-proof` |
| `burn_to_sink.hex` | grouped asset → protocol NUMS **burn sink** (§5.D16) | **`make recapture-vectors`** (`capture_burn_vector`) | `satusd-ta-proof`, `satusd-state::{claim,node}`, `satusd-challenger`, `satusd-dispute` |
| `lock_anchor.hex` | grouped asset → §5.D3 **lock anchor** (`derive_lock_script_key(intent)` + NUMS internal + finalize/refund sibling, §5.D15) | **`make recapture-vectors`** (`capture_lock_vector`) | `satusd-ta-proof`, `satusd-state::{redeem,node}` |

The two most-depended-on fixtures (`burn_to_sink`, `lock_anchor`) have capture bins
and are automated by `make recapture-vectors`. The four single-/grouped-transfer
fixtures were captured ad-hoc during M3 (tapd `ExportProof` + the Go oracle in
`tools/tap-proof-oracle/`); they exercise the lineage verifier's parsing, not the
protocol bindings, so they change less often — recapture them manually (below) only
when the asset they describe is rebuilt.

## Pinned constants (update these after a recapture)

When the devnet asset changes, `capture_burn_vector` prints the new values; update:

| Constant | Where | = |
|---|---|---|
| `FAMILY` | `crates/satusd-state/src/claim.rs` (test), `crates/satusd-state/src/node.rs` (test) | `family_id` printed by `capture_burn_vector` |
| burn sink script key | `crates/satusd-ta-proof/src/lib.rs` → `burn_to_sink_lineage_verifies` (the `c2945baf…` assertion) | `sink_key` printed by `capture_burn_vector` |
| burn amount / leaf amount | `satusd-ta-proof` `burn_to_sink_lineage_verifies` (`leaf.amount`) | `BURN_AMOUNT` in `capture_burn_vector` |
| canonical `RedeemIntent` | `capture_lock_vector` ↔ mirrored verbatim by `satusd-state::redeem`'s `apply_redeem_lock` test | must stay byte-identical in both |

> `satusd-state::redeem`'s test `FAMILY` is the synthetic `[0x01; 32]` (it tests the
> binding logic, not a real captured family); the *real* family lives in `claim.rs` /
> `node.rs`. Keep them straight.

## Recapture procedure

1. **Rebuild devnet**: `make devnet-down && make devnet-up` (bitcoind + lnd + tapd;
   see `devnet/README.md`). Mint a **grouped** SatUSD asset (the protocol uses a
   grouped family; `capture_*` compute the family from the group key + genesis id).
2. **Automated fixtures**: `make recapture-vectors` → regenerates `burn_to_sink.hex`
   + `lock_anchor.hex` and prints `family_id` / `sink_key` / etc.
3. **Manual fixtures** (only if rebuilding the single-/grouped-asset lineage too):
   recapture `genesis/transfer/grouped_transfer/burn_transfer.hex` via tapd
   `ExportProof` after the corresponding mint/transfer steps — the Go oracle in
   `tools/tap-proof-oracle/` (see its `README.md`) decodes/exports the raw bytes; the
   `wallet` bin (`satusd-tapd-client`) drives the transfers.
4. **Update the pinned constants** above from the printed values.
5. **Verify**: `make vectors && make test`. Any remaining mismatch (family, script
   key, amount, intent) fails a `satusd-ta-proof` / `satusd-state` test loudly — that
   is the safety net, not a regression.

## Why pinned (not derived) at test time

The fixtures are opaque tapd bytes; the verifier re-derives the family / sink / lock
key from them and the protocol rules and asserts equality against the pinned
constants. Pinning is what makes a wrong derivation (or a fixture swapped for a
different asset) detectable — a fully self-referential test would pass against any
internally-consistent garbage. The cost is this recapture coupling, documented here.
