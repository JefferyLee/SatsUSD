// TS port of the vault contract + settlement math (crates/satusd-vault
// contract.rs + settle.rs, spec 06 §2–5). Lets the phone compute and
// show a vault's terms — collateral ratio, the crash threshold, and the
// liquidation payout — on-device, and REFUSE an under-collateralised
// open, before any node opens the position. Integer math (BigInt),
// rounding toward the reserve, to byte-match the Rust.
import { taggedHash } from "./rail.ts";
import { bytesToHex } from "./encoder.ts";

const SATS_PER_BTC = 100_000_000n;
const MICRO_PER_USD = 1_000_000n;
const U32_MAX = (1n << 32n) - 1n;

export interface VaultTerms {
  collateralSats: bigint; // BTC locked in the funding output
  mintMicroUsd: bigint; // face value minted, µUSD (1 SatUSD = 1e6 µUSD)
  openingCrBps: number; // min opening CR (e.g. 15000 = 150%)
  liqCrBps: number; // liquidation CR (e.g. 11000 = 110%)
  checkpointInterval: number; // N blocks
  maturityHeight: number;
  m: number; // CET bucket prefix bits
  penaltyBps: number; // liquidation penalty, bps of face
  oracleEventSeries: Uint8Array; // 32-byte series id
}

export interface PayoutParams {
  penaltyBps: number;
  bountyBpsOfPenalty: number;
  bountyCapSats: bigint;
  feeBudgetSats: bigint;
}

/** spec 06 §8 conservative starting values. */
export const DEFAULT_TERMS = (collateralSats: bigint, mintMicroUsd: bigint): VaultTerms => ({
  collateralSats,
  mintMicroUsd,
  openingCrBps: 15_000,
  liqCrBps: 11_000,
  checkpointInterval: 144,
  maturityHeight: 1_000_000,
  m: 9,
  penaltyBps: 500,
  oracleEventSeries: new Uint8Array(32),
});

export const DEFAULT_PARAMS: PayoutParams = {
  penaltyBps: 500,
  bountyBpsOfPenalty: 1_000,
  bountyCapSats: 5_000n,
  feeBudgetSats: 2_000n,
};

const minBig = (a: bigint, b: bigint) => (a < b ? a : b);
const satClamp = (x: bigint) => (x < 0n ? 0n : x);

/** Collateral ratio in basis points at a BTC/USD price (truncating —
 *  the conservative read). `CR = collateral_value / face`. */
export function crBps(collateralSats: bigint, mintMicroUsd: bigint, priceUsd: number): number {
  if (mintMicroUsd === 0n) return Number(U32_MAX);
  const num = collateralSats * BigInt(priceUsd) * MICRO_PER_USD * 10_000n;
  const den = SATS_PER_BTC * mintMicroUsd;
  return Number(minBig(num / den, U32_MAX));
}

export function openingOk(terms: VaultTerms, priceUsd: number): boolean {
  return crBps(terms.collateralSats, terms.mintMicroUsd, priceUsd) >= terms.openingCrBps;
}

/** Highest BTC/USD price still in crash territory (own CR ≤ liq_CR);
 *  prices at or below this make a crash CET broadcastable. Floors. */
export function crashPriceCeiling(terms: VaultTerms): number {
  if (terms.collateralSats === 0n) return Number(U32_MAX);
  const num = BigInt(terms.liqCrBps) * terms.mintMicroUsd * SATS_PER_BTC;
  const den = terms.collateralSats * MICRO_PER_USD * 10_000n;
  if (den === 0n) return Number(U32_MAX);
  return Number(minBig(num / den, U32_MAX));
}

/** Face value in sats at a price — what the reserve is owed. Rounds UP. */
export function faceSats(mintMicroUsd: bigint, priceUsd: number): bigint {
  if (priceUsd === 0) return (1n << 64n) - 1n;
  const num = mintMicroUsd * SATS_PER_BTC;
  const den = BigInt(priceUsd) * MICRO_PER_USD;
  return (num + den - 1n) / den;
}

export interface CrashPayout {
  reserveSats: bigint;
  broadcasterSats: bigint;
  minterSats: bigint;
}

/** Split a settlement at price `p`; outputs sum to collateral − fee.
 *  Reserve is clamped in a deep gap (the cushion the pool absorbs). */
export function crashPayout(
  collateralSats: bigint,
  mintMicroUsd: bigint,
  priceUsd: number,
  p: PayoutParams,
): CrashPayout {
  const face = faceSats(mintMicroUsd, priceUsd);
  const penalty = (face * BigInt(p.penaltyBps)) / 10_000n;
  let bounty = (penalty * BigInt(p.bountyBpsOfPenalty)) / 10_000n;
  bounty = minBig(bounty, p.bountyCapSats);

  const spendable = satClamp(collateralSats - p.feeBudgetSats);
  bounty = minBig(bounty, spendable);

  const reserveWant = satClamp(face + penalty - bounty);
  const reserve = minBig(reserveWant, spendable - bounty);
  const minter = spendable - bounty - reserve;
  return { reserveSats: reserve, broadcasterSats: bounty, minterSats: minter };
}

/** The reserve's shortfall vs the face it is owed (gap the pool absorbs). */
export function reserveShortfallSats(po: CrashPayout, mintMicroUsd: bigint, priceUsd: number): bigint {
  return satClamp(faceSats(mintMicroUsd, priceUsd) - po.reserveSats);
}

function beBytes(n: bigint, len: number): Uint8Array {
  const out = new Uint8Array(len);
  for (let i = len - 1; i >= 0; i--) { out[i] = Number(n & 0xffn); n >>= 8n; }
  return out;
}

/** Canonical vault_id = TaggedHash("SatUSD/vault-contract/v1", encode(terms)),
 *  byte-matching contract.rs's fixed big-endian field layout. */
export function vaultId(t: VaultTerms): string {
  const parts: Uint8Array[] = [
    beBytes(t.collateralSats, 8),
    beBytes(t.mintMicroUsd, 8),
    beBytes(BigInt(t.openingCrBps), 4),
    beBytes(BigInt(t.liqCrBps), 4),
    beBytes(BigInt(t.checkpointInterval), 4),
    beBytes(BigInt(t.maturityHeight), 4),
    new Uint8Array([t.m]),
    beBytes(BigInt(t.penaltyBps), 4),
    t.oracleEventSeries,
  ];
  const len = parts.reduce((a, p) => a + p.length, 0);
  const buf = new Uint8Array(len);
  let o = 0;
  for (const p of parts) { buf.set(p, o); o += p.length; }
  return bytesToHex(taggedHash("SatUSD/vault-contract/v1", buf));
}
