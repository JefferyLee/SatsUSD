// TypeScript reference for the rebuilt (MISSION v2) protocol surface —
// must byte-match the Rust crates: satusd-rail, satusd-rail0,
// satusd-rail1, satusd-oracle, satusd-allot, satusd-verify.
// Independent reimplementation: spec 00 §3 encodings, BIP-340 signing
// with predetermined nonces, DLC anticipation points, taproot tree math.

import { createHash } from "node:crypto";
import * as secpMod from "@noble/curves/secp256k1.js";
import { hexToBytes, bytesToHex } from "./encoder.ts";

const secp = secpMod.secp256k1;
const Point = (secp as any).Point ?? (secp as any).ProjectivePoint;
const N: bigint = (secp as any).CURVE?.n ?? Point.Fn.ORDER;

const U64_MAX = (1n << 64n) - 1n;

function sha256(...parts: Uint8Array[]): Uint8Array {
  const h = createHash("sha256");
  for (const p of parts) h.update(p);
  return new Uint8Array(h.digest());
}

function utf8(s: string): Uint8Array {
  return new Uint8Array(Buffer.from(s, "utf8"));
}

export function taggedHash(tag: string, msg: Uint8Array): Uint8Array {
  const th = sha256(utf8(tag));
  return sha256(th, th, msg);
}

function concat(...parts: Uint8Array[]): Uint8Array {
  const len = parts.reduce((a, p) => a + p.length, 0);
  const out = new Uint8Array(len);
  let off = 0;
  for (const p of parts) {
    out.set(p, off);
    off += p.length;
  }
  return out;
}

function toHex32(x: bigint): string {
  return x.toString(16).padStart(64, "0");
}

function big(hexOrDec: string): bigint {
  return BigInt(hexOrDec);
}

function bytesToBig(b: Uint8Array): bigint {
  return BigInt("0x" + (bytesToHex(b) || "0"));
}

// ---- spec 00 §3 canonical encoder (big-endian, BigSize) ----

export class RailEncoder {
  private parts: number[] = [];

  bytes(): Uint8Array {
    return Uint8Array.from(this.parts);
  }

  u8(v: number): void {
    this.parts.push(v & 0xff);
  }

  bool(v: boolean): void {
    this.parts.push(v ? 1 : 0);
  }

  u16(v: number): void {
    this.parts.push((v >>> 8) & 0xff, v & 0xff);
  }

  u32(v: number): void {
    this.parts.push((v >>> 24) & 0xff, (v >>> 16) & 0xff, (v >>> 8) & 0xff, v & 0xff);
  }

  // i32 big-endian two's complement (JS bitwise ops are 32-bit).
  i32(v: number): void {
    this.u32(v >>> 0);
  }

  u64(v: bigint): void {
    for (let shift = 56n; shift >= 0n; shift -= 8n) {
      this.parts.push(Number((v >> shift) & 0xffn));
    }
  }

  bytes32(hex: string): void {
    this.raw(hexToBytes(hex));
  }

  raw(b: Uint8Array): void {
    for (const x of b) this.parts.push(x);
  }

  bigsize(v: bigint): void {
    if (v <= 0xfcn) {
      this.parts.push(Number(v));
    } else if (v <= 0xffffn) {
      this.parts.push(0xfd);
      this.u16(Number(v));
    } else if (v <= 0xffff_ffffn) {
      this.parts.push(0xfe);
      this.u32(Number(v));
    } else {
      this.parts.push(0xff);
      this.u64(v);
    }
  }

  varbytes(b: Uint8Array): void {
    this.bigsize(BigInt(b.length));
    this.raw(b);
  }

  string(s: string): void {
    this.varbytes(utf8(s));
  }
}

export function bigsizeHex(value: string): string {
  const e = new RailEncoder();
  e.bigsize(big(value));
  return bytesToHex(e.bytes());
}

// ---- RailManifest (spec 02 §2) ----

function encodeEventTemplate(e: RailEncoder, t: any): void {
  e.string(t.pair);
  e.u32(t.cadence_s);
  e.u16(t.base);
  e.u16(t.nb_digits);
  e.bool(t.is_signed);
  e.i32(t.precision);
  e.string(t.unit);
}

function encodeOracleSpec(e: RailEncoder, o: any): void {
  e.u8(o.variant);
  if (o.variant === 1) {
    e.u16(o.k);
    e.u16(o.n);
    e.bigsize(BigInt(o.oracle_pubkeys.length));
    for (const pk of o.oracle_pubkeys) e.bytes32(pk);
    encodeEventTemplate(e, o.event_template);
  } else if (o.variant === 2) {
    e.u32(o.window_blocks);
    e.u64(big(o.bond_sats));
  } else if (o.variant === 3) {
    e.u32(o.window_blocks);
    e.u64(big(o.min_volume_usd));
  }
}

function encodeLiveness(e: RailEncoder, l: any): void {
  e.u8(l.variant);
  if (l.variant === 1) {
    e.bytes32(l.beacon);
    e.u32(l.delay_s);
  } else if (l.variant === 2) {
    e.varbytes(hexToBytes(l.params));
  }
}

export function encodeManifest(f: any): Uint8Array {
  const e = new RailEncoder();
  e.u16(f.spec_version);
  e.u8(f.direction);
  e.bytes32(f.asset_id);
  encodeOracleSpec(e, f.oracle_spec);
  e.u8(f.settle_primitive);
  e.u64(big(f.max_size_sats));
  e.u16(f.fee_schedule.retain_bps);
  e.u16(f.fee_schedule.service_bps);
  e.u64(big(f.fee_schedule.fixed_sats));
  e.u16(f.price_dev_bound_bps);
  e.u32(f.quote_validity_s);
  e.u32(f.settle_window_s);
  e.u32(f.refund_delta_blocks);
  e.u64(big(f.lp_stake.amount_sats));
  e.bigsize(BigInt(f.lp_stake.slash_conditions.length));
  for (const c of f.lp_stake.slash_conditions) e.bytes32(c);
  e.bytes32(f.dispute_hook);
  encodeLiveness(e, f.liveness_escape);
  const tiers = [...f.automation_tiers].sort((a: number[], b: number[]) => a[0] - b[0]);
  e.bigsize(BigInt(tiers.length));
  for (const [t, tier] of tiers) {
    e.u8(t);
    e.u8(tier);
  }
  return e.bytes();
}

export function railIdHex(f: any): string {
  return bytesToHex(taggedHash("SatUSD/rail-manifest/v1", encodeManifest(f)));
}

// ---- AssetMeta (spec 01 §2.2) ----

export function encodeAssetMeta(f: any): Uint8Array {
  const e = new RailEncoder();
  e.u16(f.spec_version);
  e.string(f.name);
  e.u8(f.decimals);
  e.bytes32(f.mission_commitment);
  return e.bytes();
}

// ---- Quote (spec 02 §3.1) ----

export function encodeQuote(f: any): Uint8Array {
  const e = new RailEncoder();
  e.bytes32(f.rail_id);
  e.u64(big(f.price_msat_per_usd));
  e.u64(big(f.amount_micro_usd));
  e.u64(big(f.expiry_unix_s));
  e.bytes32(f.lock_template_commitment);
  return e.bytes();
}

export function quoteSigningPayloadHex(f: any): string {
  return bytesToHex(taggedHash("SatUSD/quote/v1", encodeQuote(f)));
}

// Floored, saturating at u64::MAX (spec 00 §3.7 rounding; adversarial
// price×amount must surface as cap-violating, never small).
export function quoteUserSats(f: any): bigint {
  const prod = big(f.amount_micro_usd) * big(f.price_msat_per_usd);
  const sats = prod / 1_000_000_000n;
  return sats > U64_MAX ? U64_MAX : sats;
}

// ---- SwapPlan (Rail-0 fee math) ----

function feeRoundHalfUp(amount: bigint, bps: number): bigint {
  return (amount * BigInt(bps) + 5_000n) / 10_000n;
}

export function swapPlanFromQuote(manifest: any, quote: any): any {
  if (manifest.oracle_spec.variant !== 0 || manifest.settle_primitive !== 0) {
    return { error: "NotRail0" };
  }
  if (railIdHex(manifest) !== quote.rail_id) {
    return { error: "RailIdMismatch" };
  }
  const gross = quoteUserSats(quote);
  if (gross > big(manifest.max_size_sats)) {
    return { error: "OverMaxSize" };
  }
  const service = feeRoundHalfUp(gross, manifest.fee_schedule.service_bps);
  const user = gross - service - big(manifest.fee_schedule.fixed_sats);
  if (user < 546n) {
    return { error: "DustPayout" };
  }
  const retained = feeRoundHalfUp(gross, manifest.fee_schedule.retain_bps);
  const e = new RailEncoder();
  e.bytes32(quote.rail_id);
  e.u64(big(quote.amount_micro_usd));
  e.u64(gross);
  e.u64(service);
  e.u64(user);
  e.u64(retained);
  return {
    burn_micro_usd: big(quote.amount_micro_usd).toString(),
    gross_sats: gross.toString(),
    service_fee_sats: service.toString(),
    user_sats: user.toString(),
    retained_sats: retained.toString(),
    lock_template_commitment: bytesToHex(
      taggedHash("SatUSD/rail0-lock-template/v1", e.bytes()),
    ),
  };
}

// ---- capacity (spec 02 §6.1, α = 1/2) ----

export function capacitySats(fees: bigint, stake: bigint): bigint {
  return (fees + stake) / 2n;
}

// ---- allotment plan (spec 04 §2, FR-6) ----

export function computeAllotment(input: any): any {
  const positions = [...input.positions].sort((a: any, b: any) =>
    a.rail_id < b.rail_id ? -1 : a.rail_id > b.rail_id ? 1 : 0,
  );
  for (let i = 1; i < positions.length; i++) {
    if (positions[i].rail_id === positions[i - 1].rail_id) {
      return { error: "DuplicateRailId" };
    }
  }
  const reserve = big(input.reserve_total_sats);

  const ei = new RailEncoder();
  ei.u32(input.epoch);
  ei.u64(reserve);
  ei.bigsize(BigInt(positions.length));
  for (const p of positions) {
    ei.bytes32(p.rail_id);
    ei.u64(big(p.retained_fees_cum_sats));
    ei.u64(big(p.lp_stake_sats));
  }
  const inputHash = taggedHash("SatUSD/allotment-input/v1", ei.bytes());

  const caps = positions.map((p: any) =>
    capacitySats(big(p.retained_fees_cum_sats), big(p.lp_stake_sats)),
  );
  const totalCap = caps.reduce((a: bigint, c: bigint) => a + c, 0n);
  const tranches = positions.map((p: any, i: number) => {
    const tranche =
      totalCap <= reserve || totalCap === 0n ? caps[i] : (caps[i] * reserve) / totalCap;
    return { rail_id: p.rail_id, capacity: caps[i], tranche };
  });
  const allocated = tranches.reduce((a: bigint, t: any) => a + t.tranche, 0n);
  const unallocated = reserve > allocated ? reserve - allocated : 0n;

  const ep = new RailEncoder();
  ep.u32(input.epoch);
  ep.u64(reserve);
  ep.raw(inputHash);
  ep.bigsize(BigInt(tranches.length));
  for (const t of tranches) {
    ep.bytes32(t.rail_id);
    ep.u64(t.capacity);
    ep.u64(t.tranche);
  }
  ep.u64(unallocated);
  const encoding = ep.bytes();

  return {
    input_hash: bytesToHex(inputHash),
    encoding_hex: bytesToHex(encoding),
    plan_hash: bytesToHex(taggedHash("SatUSD/allotment-plan/v1", encoding)),
    unallocated_sats: unallocated.toString(),
  };
}

// ---- taproot tweaks / tapd burn key (spec 01 §4, proposal 0001) ----

function liftEven(xHex: string): any {
  return Point.fromHex("02" + xHex);
}

function pointHex33(P: any): string {
  const b = P.toBytes ? P.toBytes(true) : P.toRawBytes(true);
  return bytesToHex(b);
}

export function tapTweakDataHex(internalXHex: string, data: Uint8Array): string {
  const P = liftEven(internalXHex);
  const t = bytesToBig(taggedHash("TapTweak", concat(hexToBytes(internalXHex), data))) % N;
  const Q = P.add(Point.BASE.multiply(t));
  return toHex32(Q.toAffine().x);
}

export const TAPD_NUMS_X = "7c79b9b26e463895eef5679d8558942c86c4ad2233adef01bc3e6d540b3653fe";

export function deriveBurnKeyHex(
  txidWireHex: string,
  vout: number,
  assetIdHex: string,
  scriptKeyHex: string,
): string {
  const voutLe = new Uint8Array([
    vout & 0xff,
    (vout >>> 8) & 0xff,
    (vout >>> 16) & 0xff,
    (vout >>> 24) & 0xff,
  ]);
  const data = concat(
    hexToBytes(txidWireHex),
    voutLe,
    hexToBytes(assetIdHex),
    hexToBytes(scriptKeyHex),
  );
  return tapTweakDataHex(TAPD_NUMS_X, data);
}

// ---- oracle (spec 03 §3, dlcspecs) ----

export const NB_DIGITS = 20;
const MAX_OUTCOME = (1 << NB_DIGITS) - 1;

export function eventId(unixTs: string): string {
  return `SatUSD/BTCUSD/${unixTs}`;
}

export function digits(priceUsd: number): number[] {
  const clamped = Math.min(priceUsd, MAX_OUTCOME);
  const out: number[] = [];
  for (let i = 0; i < NB_DIGITS; i++) {
    out.push((clamped >>> (NB_DIGITS - 1 - i)) & 1);
  }
  return out;
}

export function priceFromDigits(ds: number[]): number {
  return ds.reduce((acc, d) => (acc << 1) | d, 0) >>> 0;
}

export function oracleNonce(seedHex: string, id: string, digitIndex: number): Uint8Array {
  const idx = new Uint8Array([(digitIndex >>> 8) & 0xff, digitIndex & 0xff]);
  return taggedHash("SatUSD/oracle/nonce/v1", concat(hexToBytes(seedHex), utf8(id), idx));
}

export function attestationMsg(digit: number): Uint8Array {
  return taggedHash("DLC/oracle/attestation/v0", utf8(digit === 0 ? "0" : "1"));
}

function scalarModN(b: Uint8Array): bigint {
  return bytesToBig(b) % N;
}

function challenge(rxHex: string, pxHex: string, msg: Uint8Array): bigint {
  return scalarModN(
    taggedHash("BIP0340/challenge", concat(hexToBytes(rxHex), hexToBytes(pxHex), msg)),
  );
}

function evenScalarAndX(scalar: bigint): { d: bigint; x: string } {
  const P = Point.BASE.multiply(scalar);
  const a = P.toAffine();
  return { d: a.y % 2n === 1n ? N - scalar : scalar, x: toHex32(a.x) };
}

// BIP-340 with a predetermined nonce: s = k + e·d (even-Y rules).
export function signWithNonce(sk: Uint8Array, k: Uint8Array, msg: Uint8Array): Uint8Array {
  const { d, x: px } = evenScalarAndX(bytesToBig(sk));
  const { d: kk, x: rx } = evenScalarAndX(bytesToBig(k));
  const e = challenge(rx, px, msg);
  const s = (kk + ((e * d) % N)) % N;
  return concat(hexToBytes(rx), hexToBytes(toHex32(s)));
}

export function noncePointHex(k: Uint8Array): string {
  return toHex32(Point.BASE.multiply(bytesToBig(k)).toAffine().x);
}

// Plain BIP-340 verification: s·G == R + e·P.
export function schnorrVerify(pubkeyHex: string, msg: Uint8Array, sig: Uint8Array): boolean {
  try {
    const rx = bytesToHex(sig.slice(0, 32));
    const s = bytesToBig(sig.slice(32));
    if (s >= N) return false;
    const e = challenge(rx, pubkeyHex, msg);
    const left = Point.BASE.multiply(s);
    const right = liftEven(rx).add(liftEven(pubkeyHex).multiply(e));
    return left.equals(right);
  } catch {
    return false;
  }
}

// S = R + e·P — the anticipated signature point (DLC core).
export function anticipationPoint(rxHex: string, pxHex: string, msg: Uint8Array): any {
  const e = challenge(rxHex, pxHex, msg);
  return liftEven(rxHex).add(liftEven(pxHex).multiply(e));
}

// dlcspecs TLVs (Messaging.md wire layout).
const TYPE_ORACLE_EVENT = 55330n;
const TYPE_ORACLE_ANNOUNCEMENT = 55332n;
const TYPE_ORACLE_ATTESTATION = 55400n;
const TYPE_DIGIT_DESCRIPTOR = 55306n;

function tlv(typeId: bigint, value: Uint8Array): Uint8Array {
  const e = new RailEncoder();
  e.bigsize(typeId);
  e.bigsize(BigInt(value.length));
  e.raw(value);
  return e.bytes();
}

function digitDescriptorTlv(): Uint8Array {
  const v = new RailEncoder();
  v.u16(2); // base
  v.bool(false);
  v.string("USD/BTC");
  v.i32(0);
  v.u16(NB_DIGITS);
  return tlv(TYPE_DIGIT_DESCRIPTOR, v.bytes());
}

function oracleEventTlv(
  noncePoints: string[],
  maturityUnix: number,
  descriptor: Uint8Array,
  id: string,
): Uint8Array {
  const v = new RailEncoder();
  v.u16(noncePoints.length);
  for (const n of noncePoints) v.bytes32(n);
  v.u32(maturityUnix);
  v.raw(descriptor);
  v.string(id);
  return tlv(TYPE_ORACLE_EVENT, v.bytes());
}

export interface TsOracle {
  sk: Uint8Array;
  seedHex: string;
  pubkeyHex: string;
}

export function oracleFromSeed(seedHex: string): TsOracle {
  const sk = taggedHash("SatUSD/oracle/key/v1", hexToBytes(seedHex));
  const { x } = evenScalarAndX(bytesToBig(sk));
  return { sk, seedHex, pubkeyHex: x };
}

export function announce(
  o: TsOracle,
  unixTs: string,
): { tlvHex: string; noncePoints: string[]; eventId: string } {
  const id = eventId(unixTs);
  const points: string[] = [];
  for (let i = 0; i < NB_DIGITS; i++) {
    points.push(noncePointHex(oracleNonce(o.seedHex, id, i)));
  }
  const event = oracleEventTlv(points, Number(unixTs) >>> 0, digitDescriptorTlv(), id);
  const msg = taggedHash("DLC/oracle/announcement/v0", event);
  const annNonce = taggedHash(
    "SatUSD/oracle/announcement-nonce/v1",
    concat(hexToBytes(o.seedHex), utf8(id)),
  );
  const sig = signWithNonce(o.sk, annNonce, msg);
  const v = new RailEncoder();
  v.raw(sig);
  v.bytes32(o.pubkeyHex);
  v.raw(event);
  return { tlvHex: bytesToHex(tlv(TYPE_ORACLE_ANNOUNCEMENT, v.bytes())), noncePoints: points, eventId: id };
}

export function attest(
  o: TsOracle,
  unixTs: string,
  priceUsd: number,
): { tlvHex: string; signatures: Uint8Array[] } {
  const id = eventId(unixTs);
  const ds = digits(priceUsd);
  const sigs: Uint8Array[] = [];
  for (let i = 0; i < ds.length; i++) {
    sigs.push(signWithNonce(o.sk, oracleNonce(o.seedHex, id, i), attestationMsg(ds[i])));
  }
  const v = new RailEncoder();
  v.string(id);
  v.bytes32(o.pubkeyHex);
  v.u16(sigs.length);
  for (const s of sigs) v.raw(s);
  for (const dgt of ds) v.string(dgt === 0 ? "0" : "1");
  return { tlvHex: bytesToHex(tlv(TYPE_ORACLE_ATTESTATION, v.bytes())), signatures: sigs };
}

// Per-digit anticipation contract: S_i = R_i + e_i·P must equal s_i·G.
export function anticipationHolds(
  noncePoints: string[],
  pubkeyHex: string,
  signatures: Uint8Array[],
  priceUsd: number,
): boolean {
  const ds = digits(priceUsd);
  for (let i = 0; i < ds.length; i++) {
    const sig = signatures[i];
    if (bytesToHex(sig.slice(0, 32)) !== noncePoints[i]) return false;
    const S = anticipationPoint(noncePoints[i], pubkeyHex, attestationMsg(ds[i]));
    const sG = Point.BASE.multiply(bytesToBig(sig.slice(32)));
    if (!S.equals(sG)) return false;
  }
  return true;
}

// ---- CET buckets (Rail-1) ----

export function bucketOf(priceUsd: number, m: number): number {
  const clamped = Math.min(priceUsd, MAX_OUTCOME);
  return clamped >>> (NB_DIGITS - m);
}

export function bucketSchedule(m: number, factor: bigint): any[] {
  const width = 1 << (NB_DIGITS - m);
  const out: any[] = [];
  for (let index = 0; index < 1 << m; index++) {
    const lo = index * width;
    out.push([index, lo, lo + width, (BigInt(lo + (width >>> 1)) * factor).toString()]);
  }
  return out;
}

export function bucketAdaptorPointHex(
  noncePoints: string[],
  pubkeyHex: string,
  m: number,
  bucketIndex: number,
): string {
  let acc: any = null;
  for (let i = 0; i < m; i++) {
    const digit = (bucketIndex >>> (m - 1 - i)) & 1;
    const S = anticipationPoint(noncePoints[i], pubkeyHex, attestationMsg(digit));
    acc = acc === null ? S : acc.add(S);
  }
  return pointHex33(acc);
}

export function bucketSecretHex(signatures: Uint8Array[], m: number): string {
  let acc = 0n;
  for (let i = 0; i < m; i++) {
    acc = (acc + bytesToBig(signatures[i].slice(32))) % N;
  }
  return toHex32(acc);
}

export function secretMatchesPoint(secretHex: string, pointHex: string): boolean {
  return pointHex33(Point.BASE.multiply(BigInt("0x" + secretHex))) === pointHex;
}

// ---- internal_twap marker (spec 03 §5.2) ----

export function internalTwap(
  points: [string, string][],
  minVolume: string,
): string | null {
  if (points.length === 0) return null;
  const parsed = points.map(([p, v]) => ({ price: big(p), vol: big(v) }));
  const prices = parsed.map((p) => p.price).sort((a, b) => (a < b ? -1 : a > b ? 1 : 0));
  const median = prices[(prices.length - 1) >> 1];
  const band = (median * 500n) / 10_000n;
  const survivors = parsed.filter((p) => {
    const diff = p.price > median ? p.price - median : median - p.price;
    return diff <= band;
  });
  const total = survivors.reduce((a, p) => a + p.vol, 0n);
  if (total < big(minVolume) || total === 0n) return null;
  survivors.sort((a, b) => (a.price < b.price ? -1 : a.price > b.price ? 1 : 0));
  const half = (total + 1n) / 2n;
  let acc = 0n;
  for (const p of survivors) {
    acc += p.vol;
    if (acc >= half) return p.price.toString();
  }
  throw new Error("unreachable: cumulative volume reaches total");
}

// ---- Rail-1 funding output (proposal 0001 §3.1) ----

function compactSize(len: number): Uint8Array {
  if (len <= 0xfc) return new Uint8Array([len]);
  if (len <= 0xffff) return new Uint8Array([0xfd, len & 0xff, (len >>> 8) & 0xff]);
  return new Uint8Array([
    0xfe,
    len & 0xff,
    (len >>> 8) & 0xff,
    (len >>> 16) & 0xff,
    (len >>> 24) & 0xff,
  ]);
}

export function tapLeafHash(script: Uint8Array): Uint8Array {
  return taggedHash("TapLeaf", concat(new Uint8Array([0xc0]), compactSize(script.length), script));
}

function tapBranch(aHex: string, bHex: string): Uint8Array {
  const [lo, hi] = aHex < bHex ? [aHex, bHex] : [bHex, aHex];
  return taggedHash("TapBranch", concat(hexToBytes(lo), hexToBytes(hi)));
}

export function fundingOutput(
  internalXHex: string,
  taLeafHex: string,
  scriptHex: string,
): { merkleRoot: string; outputX: string } {
  const refundLeaf = bytesToHex(tapLeafHash(hexToBytes(scriptHex)));
  const root = tapBranch(taLeafHex, refundLeaf);
  return {
    merkleRoot: bytesToHex(root),
    outputX: tapTweakDataHex(internalXHex, root),
  };
}

export function siblingPreimageHex(scriptHex: string): string {
  const script = hexToBytes(scriptHex);
  return bytesToHex(
    concat(new Uint8Array([0x00, 0xc0]), compactSize(script.length), script),
  );
}
