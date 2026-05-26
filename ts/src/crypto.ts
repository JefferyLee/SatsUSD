// TypeScript reference for SatUSD crypto primitives — must byte-match satusd-crypto.
// Poseidon via circomlibjs (circomlib BN254, ADR-0013); secp256k1 via @noble/curves.

import { createHash } from "node:crypto";
import { buildPoseidon } from "circomlibjs";
import * as secpMod from "@noble/curves/secp256k1.js";
import { hexToBytes, bytesToHex } from "./encoder.ts";

const secp = secpMod.secp256k1;
const Point = (secp as any).Point ?? (secp as any).ProjectivePoint;
const N: bigint = (secp as any).CURVE?.n ?? Point.Fn.ORDER;

// circomlibjs builds its hasher asynchronously; do it once at import time.
const poseidon: any = await buildPoseidon();
const F = poseidon.F;

function toHex32(x: bigint): string {
  return x.toString(16).padStart(64, "0");
}

function sha256(...parts: Uint8Array[]): Uint8Array {
  const h = createHash("sha256");
  for (const p of parts) h.update(p);
  return new Uint8Array(h.digest());
}

// ---- Poseidon ----

function poseidon2Fr(a: bigint, b: bigint): bigint {
  return BigInt(F.toString(poseidon([a, b])));
}

export function poseidon2Hex(aHex: string, bHex: string): string {
  return toHex32(poseidon2Fr(BigInt("0x" + aHex), BigInt("0x" + bHex)));
}

export function hashBytesHex(inputHex: string): string {
  const bytes = hexToBytes(inputHex);
  let acc = 0n;
  for (let i = 0; i < bytes.length; i += 31) {
    const limb = BigInt("0x" + (bytesToHex(bytes.slice(i, i + 31)) || "0"));
    acc = poseidon2Fr(acc, limb);
  }
  return toHex32(acc);
}

// oracle_message_hash = Poseidon([set_epoch, price_epoch, timestamp_ms, price_e8])
// (§5.D7). Matches satusd_crypto::poseidon::oracle_message_hash + the M4c circuit.
export function oracleMessageHashHex(
  setEpoch: string,
  priceEpoch: string,
  timestampMs: string,
  priceE8: string,
): string {
  const out = poseidon([BigInt(setEpoch), BigInt(priceEpoch), BigInt(timestampMs), BigInt(priceE8)]);
  return toHex32(BigInt(F.toString(out)));
}

// state_root_hash fold: poseidon2-fold the 31-byte canonical-encoding limbs (acc
// starts at 0). Matches satusd_crypto::poseidon::hash_bytes + the M4 StateCommit.
export function foldLimbsHex(limbsHex: string[]): string {
  let acc = 0n;
  for (const l of limbsHex) acc = poseidon2Fr(acc, BigInt("0x" + l));
  return toHex32(acc);
}

// SMT membership fold (ADR-0015): fold `leaf` up through `siblings` (indexed by
// depth, 0 = top) applying the key's path bits MSB-first. Matches
// satusd_crypto::smt::fold_to_root and the M4b circuit `SmtFold`.
export function smtFoldHex(keyHex: string, leafHex: string, siblingsHex: string[]): string {
  const key = hexToBytes(keyHex);
  let cur = BigInt("0x" + leafHex);
  for (let d = 255; d >= 0; d--) {
    const sib = BigInt("0x" + siblingsHex[d]);
    const bit = (key[d >> 3] >> (7 - (d % 8))) & 1;
    cur = bit === 0 ? poseidon2Fr(cur, sib) : poseidon2Fr(sib, cur);
  }
  return toHex32(cur);
}

// ---- NUMS / Taproot (secp256k1) ----

function u32be(v: number): Uint8Array {
  return new Uint8Array([(v >>> 24) & 0xff, (v >>> 16) & 0xff, (v >>> 8) & 0xff, v & 0xff]);
}

function isValidXonly(x: Uint8Array): boolean {
  try {
    Point.fromHex("02" + bytesToHex(x));
    return true;
  } catch {
    return false;
  }
}

export function deriveNumsKey(domainName: string, salt: Uint8Array): Uint8Array {
  for (let ctr = 0; ; ctr++) {
    const seed = sha256(new Uint8Array(Buffer.from(domainName, "ascii")), salt, u32be(ctr));
    if (isValidXonly(seed)) return seed;
  }
}

export function numsKeyHex(domainName: string, saltHex: string): string {
  return bytesToHex(deriveNumsKey(domainName, hexToBytes(saltHex)));
}

// §5.D8 CR/tier (DL-24): cr_ppm = reserve_sats*price_e8*1e6 / (supply_atoms*1e14).
export function collateralRatioPpm(reserve: bigint, supply: bigint, price: bigint): bigint | null {
  if (supply === 0n) return null;
  return (reserve * price * 1_000_000n) / (supply * 10n ** 14n);
}

// Binary Poseidon merkle root over hex leaves (pad to power of two with zero).
export function batchRootHex(leavesHex: string[]): string {
  if (leavesHex.length === 0) return "00".repeat(32);
  let level = leavesHex.slice();
  let n = 1;
  while (n < level.length) n <<= 1;
  while (level.length < n) level.push("00".repeat(32));
  while (level.length > 1) {
    const next: string[] = [];
    for (let i = 0; i < level.length; i += 2) next.push(poseidon2Hex(level[i], level[i + 1]));
    level = next;
  }
  return level[0];
}

export function recomputeTier(reserve: bigint, supply: bigint, price: bigint): number {
  const cr = collateralRatioPpm(reserve, supply, price);
  if (cr === null || cr >= 1_500_000n) return 0; // Healthy
  if (cr >= 1_300_000n) return 1; // PauseMint
  if (cr >= 1_100_000n) return 2; // Auction
  return 3; // Settlement
}

function taggedHash(tag: string, msg: Uint8Array): Uint8Array {
  const th = sha256(new Uint8Array(Buffer.from(tag, "ascii")));
  return sha256(th, th, msg);
}

export function tapTweakHex(internalHex: string, tweakHex: string): string {
  const internal = hexToBytes(internalHex);
  const tweak = hexToBytes(tweakHex);
  const P = Point.fromHex("02" + internalHex);
  const msg = new Uint8Array([...internal, ...tweak]);
  const t = BigInt("0x" + bytesToHex(taggedHash("TapTweak", msg))) % N;
  const Q = P.add(Point.BASE.multiply(t));
  return toHex32(Q.toAffine().x);
}

export function burnTweakHex(afidHex: string): string {
  const afid = hexToBytes(afidHex);
  return bytesToHex(sha256(new Uint8Array(Buffer.from("SATUSD_BURN_TWEAK_V1", "ascii")), afid));
}

export function burnInternalKeyHex(afidHex: string): string {
  return bytesToHex(deriveNumsKey("SATUSD_BURN_SINK_V1", hexToBytes(afidHex)));
}

export function sinkScriptKeyHex(afidHex: string): string {
  return tapTweakHex(burnInternalKeyHex(afidHex), burnTweakHex(afidHex));
}
