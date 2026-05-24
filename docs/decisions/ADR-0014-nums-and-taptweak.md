# ADR-0014: NUMS key derivation and the TapTweak primitive

- **Date**: 2026-05-24
- **Status**: accepted
- **PRD reference**: §5.D3, §5.D16, §18.6, §18.7
- **Author**: Jeffery (with implementation from AI coding agent)

## Context

§18.7 gives pseudocode for `derive_nums_key` and the burn-sink derivation, and
§5.D3/§18.6 reference a `TapTweak(internal, tweak)` primitive in several places
(lock script key, HTLC output key, burn sink). The exact `TapTweak` definition is
only fully written for the HTLC (§18.6: `internal + tagged_hash("TapTweak",
internal || merkle_root) * G`). We freeze one primitive used everywhere.

## Decision

- **`derive_nums_key(domain, salt)`**: smallest `ctr` (u32, big-endian, from 0)
  such that `SHA256(domain_ascii || salt || ctr_be32)` is a valid x-only public
  key (an even-Y point lifts from it). Return that 32-byte x-only key. Domain is
  raw ASCII (ADR-0011).
- **`TapTweak(internal_xonly, tweak32)`** (BIP341): output x-only key of
  `lift_even_y(internal) + t·G` where `t = tagged_hash("TapTweak", internal_xonly
  || tweak32)` and `tagged_hash(tag, m) = SHA256(SHA256(tag) || SHA256(tag) || m)`.
- **Burn sink (§5.D16/§18.7)**:
  - `protocol_burn_internal_key = derive_nums_key("SATUSD_BURN_SINK_V1", asset_family_id)`
  - `protocol_burn_tweak = SHA256("SATUSD_BURN_TWEAK_V1" || asset_family_id)` (plain SHA256)
  - `protocol_sink_script_key = TapTweak(protocol_burn_internal_key, protocol_burn_tweak)`

Rust uses the `secp256k1` crate (`XOnlyPublicKey::from_slice` for validity,
`add_tweak` for the tweak). TS uses `@noble/curves` and byte-matches (internal +
sink keys identical across implementations).

## Consequences

- Positive: a single, BIP341-consistent `TapTweak` reused by lock key, HTLC, and
  burn sink; Rust/TS byte-match on `burn_sink` and `tap_tweak` vectors.
- Note: the secp256k1 TapTweak and NUMS derivations are off-circuit (PRD §5.D18);
  they are enforced by software verifier + challenger, and (BitVM3) dispute paths.
- Follow-up: when the lock script key (§5.D3) is implemented, reuse this exact
  `TapTweak`; add its vectors.

## References

- `crates/satusd-crypto/src/nums.rs`
- `integration/vectors/vectors.json` (`op: burn_sink`, `op: tap_tweak`)
