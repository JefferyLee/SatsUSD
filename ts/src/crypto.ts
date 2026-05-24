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
