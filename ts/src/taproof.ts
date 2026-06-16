// TS port of `satusd-ta-proof` — independent verification of a Taproot Asset
// `proof.File` lineage, so the phone can confirm a transfer itself rather than
// trust a server. Must byte-match the Rust crate; pinned against the shared
// integration/lineage_vectors/*.hex vectors (see taproof-verify.ts).
//
// Increment 1 (this file): proof.File parse + sha256 hash chain, proof.Proof
// TLV decode, genesis/asset_id. Anchor (tx-merkle → header), asset→output-key
// inclusion (MS-SMT + BIP341), and lineage continuity follow.
import { sha256 as nobleSha256 } from "@noble/hashes/sha2.js";
import { parseCompressedProof, proofRoot, leaf as mssmtLeaf } from "./mssmt.ts";
import { taggedHash, tapTweakDataHex } from "./rail.ts";
import { hexToBytes, bytesToHex } from "./encoder.ts";

export const FILE_MAGIC = "TAPF";
export const PROOF_MAGIC = "TAPP";

export type ProofErrorKind =
  | "Truncated" | "BadFileMagic" | "BadProofMagic" | "ChecksumMismatch"
  | "TrailingBytes" | "NonCanonicalBigSize" | "TlvOutOfOrder" | "BadFieldLength"
  | "BadAnchorTx" | "AnchorMerkleMismatch" | "OutputIndexOutOfRange" | "NotP2tr"
  | "CommitmentMismatch" | "UnsupportedTapVersion" | "LineageBroken" | "EmptyLineage"
  | "BadTapSibling";

export class ProofError extends Error {
  kind: ProofErrorKind;
  constructor(kind: ProofErrorKind) {
    super(kind);
    this.kind = kind;
    this.name = "ProofError";
  }
}

function sha256(...parts: Uint8Array[]): Uint8Array {
  const len = parts.reduce((a, p) => a + p.length, 0);
  const buf = new Uint8Array(len);
  let o = 0;
  for (const p of parts) { buf.set(p, o); o += p.length; }
  return nobleSha256(buf) as Uint8Array;
}

const ascii = (s: string) => new TextEncoder().encode(s);

/** lnd TLV/BigSize cursor over a byte slice, with canonical-encoding checks. */
class Cursor {
  i = 0;
  b: Uint8Array;
  constructor(b: Uint8Array) {
    this.b = b;
  }

  take(n: number): Uint8Array {
    const end = this.i + n;
    if (n < 0 || end > this.b.length) throw new ProofError("Truncated");
    const s = this.b.subarray(this.i, end);
    this.i = end;
    return s;
  }

  u32be(): number {
    const s = this.take(4);
    return ((s[0] << 24) | (s[1] << 16) | (s[2] << 8) | s[3]) >>> 0;
  }

  /** lnd BigSize varint: 1/3/5/9 bytes, big-endian, canonical (shortest) form. */
  bigsize(): bigint {
    const first = this.take(1)[0];
    if (first === 0xff) {
      const s = this.take(8);
      let x = 0n;
      for (const byte of s) x = (x << 8n) | BigInt(byte);
      if (x < 0x1_0000_0000n) throw new ProofError("NonCanonicalBigSize");
      return x;
    }
    if (first === 0xfe) {
      const x = BigInt(this.u32be());
      if (x < 0x1_0000n) throw new ProofError("NonCanonicalBigSize");
      return x;
    }
    if (first === 0xfd) {
      const s = this.take(2);
      const x = BigInt((s[0] << 8) | s[1]);
      if (x < 0xfdn) throw new ProofError("NonCanonicalBigSize");
      return x;
    }
    return BigInt(first);
  }

  /** BigSize as a JS number (for lengths/indices that fit in 2^53). */
  bigsizeNum(): number {
    return Number(this.bigsize());
  }
}

function startsWith(b: Uint8Array, magic: string): boolean {
  const m = ascii(magic);
  if (b.length < m.length) return false;
  for (let i = 0; i < m.length; i++) if (b[i] !== m[i]) return false;
  return true;
}

function eqBytes(a: Uint8Array, b: Uint8Array): boolean {
  if (a.length !== b.length) return false;
  for (let i = 0; i < a.length; i++) if (a[i] !== b[i]) return false;
  return true;
}

export interface ProofFile {
  version: number;
  proofs: Uint8Array[]; // each begins with "TAPP", lineage order (genesis first)
}

/** Parse a tapd proof.File, verifying its sha256 hash chain. */
export function parseProofFile(data: Uint8Array): ProofFile {
  const c = new Cursor(data);
  if (!eqBytes(c.take(4), ascii(FILE_MAGIC))) throw new ProofError("BadFileMagic");
  const version = c.u32be();
  const count = c.bigsizeNum();

  const proofs: Uint8Array[] = [];
  let prev: Uint8Array = new Uint8Array(32);
  for (let n = 0; n < count; n++) {
    const len = c.bigsizeNum();
    const proof = c.take(len);
    if (!startsWith(proof, PROOF_MAGIC)) throw new ProofError("BadProofMagic");
    const checksum = c.take(32);
    // tapd hash chain: h_i = sha256(h_{i-1} || proof_i), h_{-1} = 0^32.
    const h = sha256(prev, proof);
    if (!eqBytes(h, checksum)) throw new ProofError("ChecksumMismatch");
    prev = h;
    proofs.push(proof);
  }
  if (c.i !== data.length) throw new ProofError("TrailingBytes");
  return { version, proofs };
}

// Top-level proof.Proof TLV type numbers (tapd v0.7.2 proof/records.go).
export const tlv = {
  VERSION: 0, PREV_OUT: 2, BLOCK_HEADER: 4, ANCHOR_TX: 6, TX_MERKLE_PROOF: 8,
  ASSET_LEAF: 10, INCLUSION_PROOF: 12, EXCLUSION_PROOFS: 13, SPLIT_ROOT_PROOF: 15,
  META_REVEAL: 17, ADDITIONAL_INPUTS: 19, CHALLENGE_WITNESS: 21, BLOCK_HEIGHT: 22,
  GENESIS_REVEAL: 23, GROUP_KEY_REVEAL: 25, ALT_LEAVES: 27,
} as const;

export interface Proof {
  records: Array<[number, Uint8Array]>;
}

/** Parse an lnd TLV stream (BigSize type || BigSize len || value), strictly ascending. */
function parseTlv(data: Uint8Array): Array<[number, Uint8Array]> {
  const c = new Cursor(data);
  const records: Array<[number, Uint8Array]> = [];
  let last: number | null = null;
  while (c.i < data.length) {
    const ty = c.bigsizeNum();
    if (last !== null && ty <= last) throw new ProofError("TlvOutOfOrder");
    last = ty;
    const len = c.bigsizeNum();
    records.push([ty, c.take(len)]);
  }
  return records;
}

/** Parse a single proof.Proof blob ("TAPP" + TLV). */
export function parseProof(blob: Uint8Array): Proof {
  if (!startsWith(blob, PROOF_MAGIC)) {
    throw new ProofError(blob.length < 4 ? "Truncated" : "BadProofMagic");
  }
  return { records: parseTlv(blob.subarray(4)) };
}

export function proofGet(p: Proof, ty: number): Uint8Array | undefined {
  const r = p.records.find(([t]) => t === ty);
  return r ? r[1] : undefined;
}

function u32Field(p: Proof, ty: number): number | undefined {
  const v = proofGet(p, ty);
  if (!v || v.length !== 4) return undefined;
  return ((v[0] << 24) | (v[1] << 16) | (v[2] << 8) | v[3]) >>> 0;
}

export const proofVersion = (p: Proof) => u32Field(p, tlv.VERSION);
export const proofBlockHeight = (p: Proof) => u32Field(p, tlv.BLOCK_HEIGHT);

function writeBigsize(x: number, out: number[]): void {
  if (x < 0xfd) out.push(x);
  else if (x <= 0xffff) out.push(0xfd, (x >> 8) & 0xff, x & 0xff);
  else if (x <= 0xffff_ffff) out.push(0xfe, (x >>> 24) & 0xff, (x >> 16) & 0xff, (x >> 8) & 0xff, x & 0xff);
  else {
    out.push(0xff);
    for (let s = 56; s >= 0; s -= 8) out.push(Number((BigInt(x) >> BigInt(s)) & 0xffn));
  }
}

/** Re-encode a parsed proof to its canonical blob ("TAPP" + TLV) — a lossless
 *  parse must reproduce the original bytes. */
export function reencodeProof(p: Proof): Uint8Array {
  const out = [...ascii(PROOF_MAGIC)];
  for (const [ty, v] of p.records) {
    writeBigsize(ty, out);
    writeBigsize(v.length, out);
    for (const b of v) out.push(b);
  }
  return new Uint8Array(out);
}

// ---- genesis / asset leaf (asset.Asset TLV) ----

const assetTlv = {
  VERSION: 0, GENESIS: 2, TYPE: 4, AMOUNT: 6, PREV_WITNESS: 11,
  SCRIPT_VERSION: 14, SCRIPT_KEY: 16, GROUP_KEY: 17,
} as const;

export interface Genesis {
  firstPrevOutTxid: Uint8Array; // 32
  firstPrevOutVout: number;
  tag: Uint8Array;
  metaHash: Uint8Array; // 32
  outputIndex: number;
  assetType: number;
}

function u32le(n: number): Uint8Array {
  return new Uint8Array([n & 0xff, (n >> 8) & 0xff, (n >> 16) & 0xff, (n >>> 24) & 0xff]);
}
function u32be(n: number): Uint8Array {
  return new Uint8Array([(n >>> 24) & 0xff, (n >> 16) & 0xff, (n >> 8) & 0xff, n & 0xff]);
}

/** asset_id = sha256(txid ‖ u32_LE(vout) ‖ sha256(tag) ‖ meta_hash ‖
 *  u32_BE(output_index) ‖ type) — Genesis.ID() in tapd. */
export function assetId(g: Genesis): Uint8Array {
  return sha256(
    g.firstPrevOutTxid, u32le(g.firstPrevOutVout), nobleSha256(g.tag) as Uint8Array,
    g.metaHash, u32be(g.outputIndex), new Uint8Array([g.assetType]),
  );
}

function parseGenesis(v: Uint8Array): Genesis {
  const c = new Cursor(v);
  const firstPrevOutTxid = c.take(32).slice();
  const firstPrevOutVout = c.u32be();
  const tag = c.take(c.bigsizeNum()).slice();
  const metaHash = c.take(32).slice();
  const outputIndex = c.u32be();
  const assetType = c.take(1)[0];
  if (c.i !== v.length) throw new ProofError("TrailingBytes");
  return { firstPrevOutTxid, firstPrevOutVout, tag, metaHash, outputIndex, assetType };
}

export interface AssetLeaf {
  version: number;
  genesis: Genesis;
  assetType: number;
  amount: bigint;
  scriptVersion: number;
  scriptKey: Uint8Array; // 33
  groupKey?: Uint8Array;
}

function tlvMap(value: Uint8Array): Map<number, Uint8Array> {
  const m = new Map<number, Uint8Array>();
  for (const [ty, v] of parseTlv(value)) m.set(ty, v);
  return m;
}

export function parseAssetLeaf(value: Uint8Array): AssetLeaf {
  const m = tlvMap(value);
  const req = (t: number) => {
    const v = m.get(t);
    if (!v) throw new ProofError("Truncated");
    return v;
  };
  const amount = new Cursor(req(assetTlv.AMOUNT)).bigsize(); // amount is a BigSize value
  const sv = req(assetTlv.SCRIPT_VERSION);
  return {
    version: req(assetTlv.VERSION)[0],
    genesis: parseGenesis(req(assetTlv.GENESIS)),
    assetType: req(assetTlv.TYPE)[0],
    amount,
    scriptVersion: (sv[0] << 8) | sv[1],
    scriptKey: req(assetTlv.SCRIPT_KEY),
    groupKey: m.get(assetTlv.GROUP_KEY),
  };
}

// ---- TaprootProof structure ----

const taprootTlv = { OUTPUT_INDEX: 0, INTERNAL_KEY: 2, COMMITMENT_PROOF: 3, TAPSCRIPT_PROOF: 5 } as const;

export interface TaprootProof {
  outputIndex: number;
  internalKey: Uint8Array;
  commitmentProof?: Uint8Array;
  tapscriptProof?: Uint8Array;
}

export function parseTaprootProof(value: Uint8Array): TaprootProof {
  const m = tlvMap(value);
  const oi = m.get(taprootTlv.OUTPUT_INDEX);
  const ik = m.get(taprootTlv.INTERNAL_KEY);
  if (!oi || oi.length !== 4) throw new ProofError("BadFieldLength");
  if (!ik) throw new ProofError("Truncated");
  return {
    outputIndex: ((oi[0] << 24) | (oi[1] << 16) | (oi[2] << 8) | oi[3]) >>> 0,
    internalKey: ik,
    commitmentProof: m.get(taprootTlv.COMMITMENT_PROOF),
    tapscriptProof: m.get(taprootTlv.TAPSCRIPT_PROOF),
  };
}

// ---- on-chain anchoring (verify_anchor) ----

function dsha256(...parts: Uint8Array[]): Uint8Array {
  return nobleSha256(sha256(...parts)) as Uint8Array;
}

/** Bitcoin CompactSize varint (little-endian). Returns [value, newOffset]. */
function readCompact(b: Uint8Array, i: number): [number, number] {
  const first = b[i];
  if (first < 0xfd) return [first, i + 1];
  if (first === 0xfd) return [b[i + 1] | (b[i + 2] << 8), i + 3];
  if (first === 0xfe) return [(b[i + 1] | (b[i + 2] << 8) | (b[i + 3] << 16) | (b[i + 4] << 24)) >>> 0, i + 5];
  let x = 0;
  for (let k = 0; k < 8; k++) x += b[i + 1 + k] * 2 ** (8 * k);
  return [x, i + 9];
}

interface ParsedTx {
  txid: Uint8Array; // internal byte order
  outputs: Uint8Array[]; // scriptPubKeys
}

/** Parse a consensus-encoded BTC tx: extract output scripts and compute the
 *  (witness-stripped) txid = dsha256(legacy serialization), internal order. */
function parseTx(b: Uint8Array): ParsedTx {
  let i = 4; // version
  const vinStart0 = 4;
  let segwit = false;
  if (b[i] === 0x00 && b[i + 1] === 0x01) { segwit = true; i += 2; }
  const vinStart = i;
  let nIn: number;
  [nIn, i] = readCompact(b, i);
  for (let k = 0; k < nIn; k++) {
    i += 36; // prevout (txid32 + vout4)
    let sl: number;
    [sl, i] = readCompact(b, i);
    i += sl + 4; // script + sequence
  }
  let nOut: number;
  [nOut, i] = readCompact(b, i);
  const outputs: Uint8Array[] = [];
  for (let k = 0; k < nOut; k++) {
    i += 8; // value
    let sl: number;
    [sl, i] = readCompact(b, i);
    outputs.push(b.subarray(i, i + sl));
    i += sl;
  }
  const voutEnd = i;
  // legacy serialization (no marker/flag/witness): version ‖ vin..vout ‖ locktime
  const version = b.subarray(0, 4);
  const body = b.subarray(segwit ? vinStart : vinStart0, voutEnd);
  const locktime = b.subarray(b.length - 4);
  const legacy = new Uint8Array(version.length + body.length + locktime.length);
  legacy.set(version, 0);
  legacy.set(body, version.length);
  legacy.set(locktime, version.length + body.length);
  return { txid: dsha256(legacy), outputs };
}

export interface TxMerkleProof {
  nodes: Uint8Array[];
  bitsPacked: Uint8Array;
}

export function parseTxMerkleProof(value: Uint8Array): TxMerkleProof {
  const c = new Cursor(value);
  const n = c.bigsizeNum();
  const nodes: Uint8Array[] = [];
  for (let k = 0; k < n; k++) nodes.push(c.take(32));
  // Direction bits: ceil(n/8) RAW packed bytes, MSB-first (tapd packBits) — NOT
  // length-prefixed. (A 0x00 single byte makes a length-prefixed misread look
  // correct, which is why trivial regtest proofs hid this.)
  const bitsPacked = c.take(Math.ceil(n / 8));
  if (c.i !== value.length) throw new ProofError("TrailingBytes");
  return { nodes, bitsPacked };
}

/** Fold txid up through the merkle nodes to a root. bit i: 1 ⇒ current is the
 *  left child. tapd packs the bits LSB-first (`1 << (i%8)`), verified against
 *  real multi-node signet proofs — all-zero regtest bits hid the order. */
export function merkleRoot(mp: TxMerkleProof, txid: Uint8Array): Uint8Array {
  let cur = txid;
  for (let i = 0; i < mp.nodes.length; i++) {
    const byte = mp.bitsPacked[i >> 3] ?? 0;
    const left = ((byte >> (i % 8)) & 1) === 1;
    cur = left ? dsha256(cur, mp.nodes[i]) : dsha256(mp.nodes[i], cur);
  }
  return cur;
}

export interface AnchorInfo {
  txid: Uint8Array;
  outputIndex: number;
  taprootOutputKey: Uint8Array; // 32-byte x-only
}

/** Verify a proof step's on-chain anchoring: the anchor tx is committed in the
 *  block header's merkle root, and the inclusion proof's output index points at a
 *  P2TR output. Returns the anchored x-only taproot output key. */
export function verifyAnchor(p: Proof): AnchorInfo {
  const header = proofGet(p, tlv.BLOCK_HEADER);
  if (!header) throw new ProofError("Truncated");
  if (header.length !== 80) throw new ProofError("BadFieldLength");
  const merkleRootHdr = header.subarray(36, 68);

  const txBytes = proofGet(p, tlv.ANCHOR_TX);
  if (!txBytes) throw new ProofError("Truncated");
  const tx = parseTx(txBytes);

  const mpBytes = proofGet(p, tlv.TX_MERKLE_PROOF);
  if (!mpBytes) throw new ProofError("Truncated");
  const root = merkleRoot(parseTxMerkleProof(mpBytes), tx.txid);
  if (!eqBytes(root, merkleRootHdr)) throw new ProofError("AnchorMerkleMismatch");

  const inc = proofGet(p, tlv.INCLUSION_PROOF);
  if (!inc) throw new ProofError("Truncated");
  const tp = parseTaprootProof(inc);
  const spk = tx.outputs[tp.outputIndex];
  if (!spk) throw new ProofError("OutputIndexOutOfRange");
  // P2TR scriptPubKey = OP_1 (0x51) PUSH32 (0x20) <x-only key> ⇒ 34 bytes.
  if (spk.length !== 34 || spk[0] !== 0x51 || spk[1] !== 0x20) throw new ProofError("NotP2tr");
  return { txid: tx.txid, outputIndex: tp.outputIndex, taprootOutputKey: spk.subarray(2, 34) };
}

// ---- asset → output-key inclusion (DL-23) ----

const commitmentTlv = {
  ASSET_PROOF: 1, TAPROOT_ASSET_PROOF: 2, TAP_SIBLING_PREIMAGE: 5,
  AP_VERSION: 0, AP_ASSET_ID: 2, AP_MSSMT_PROOF: 4,
  TAP_VERSION: 0, TAP_MSSMT_PROOF: 2,
} as const;

export interface CommitmentProof {
  assetProofVersion: number;
  assetIdKey: Uint8Array; // the AssetCommitment's key (asset_id)
  innerMssmt: Uint8Array;
  tapVersion: number;
  outerMssmt: Uint8Array;
  tapSibling?: Uint8Array;
}

export function parseCommitmentProof(value: Uint8Array): CommitmentProof {
  const m = tlvMap(value);
  const apv = m.get(commitmentTlv.ASSET_PROOF);
  const tpv = m.get(commitmentTlv.TAPROOT_ASSET_PROOF);
  if (!apv || !tpv) throw new ProofError("Truncated");
  const ap = tlvMap(apv);
  const tp = tlvMap(tpv);
  const req = (mm: Map<number, Uint8Array>, t: number) => {
    const v = mm.get(t);
    if (!v) throw new ProofError("Truncated");
    return v;
  };
  return {
    assetProofVersion: req(ap, commitmentTlv.AP_VERSION)[0],
    assetIdKey: req(ap, commitmentTlv.AP_ASSET_ID),
    innerMssmt: req(ap, commitmentTlv.AP_MSSMT_PROOF),
    tapVersion: req(tp, commitmentTlv.TAP_VERSION)[0],
    outerMssmt: req(tp, commitmentTlv.TAP_MSSMT_PROOF),
    tapSibling: m.get(commitmentTlv.TAP_SIBLING_PREIMAGE),
  };
}

const witnessTlv = { PREV_ID: 1, TX_WITNESS: 3, SPLIT_COMMITMENT: 5 } as const;

/** For a split (transfer) output, tapd commits the asset leaf with its single
 *  split-commitment witness's SplitCommitment sub-record nulled. Re-encode with
 *  it stripped; returns null when not a split-commitment witness (raw leaf is the
 *  committed leaf, e.g. mint outputs). */
function splitCommittedLeafBytes(assetLeafBytes: Uint8Array): Uint8Array | null {
  let recs: Array<[number, Uint8Array]>;
  try { recs = parseTlv(assetLeafBytes); } catch { return null; }
  const pwRec = recs.find(([t]) => t === assetTlv.PREV_WITNESS);
  if (!pwRec) return null;
  const pw = pwRec[1];

  const c = new Cursor(pw);
  let count: bigint;
  try { count = c.bigsize(); } catch { return null; }
  if (count !== 1n) return null;
  let witness: Uint8Array;
  try { witness = c.take(c.bigsizeNum()); } catch { return null; }
  if (c.i !== pw.length) return null;

  let wrecs: Array<[number, Uint8Array]>;
  try { wrecs = parseTlv(witness); } catch { return null; }
  const has = (t: number) => wrecs.some(([ty]) => ty === t);
  if (!(has(witnessTlv.PREV_ID) && !has(witnessTlv.TX_WITNESS) && has(witnessTlv.SPLIT_COMMITMENT))) {
    return null;
  }

  const enc = (records: Array<[number, Uint8Array]>): number[] => {
    const out: number[] = [];
    for (const [ty, v] of records) {
      writeBigsize(ty, out);
      writeBigsize(v.length, out);
      for (const b of v) out.push(b);
    }
    return out;
  };
  const newWitness = enc(wrecs.filter(([ty]) => ty !== witnessTlv.SPLIT_COMMITMENT));
  const newPw: number[] = [];
  writeBigsize(1, newPw);
  writeBigsize(newWitness.length, newPw);
  newPw.push(...newWitness);

  const out: number[] = [];
  for (const [ty, v] of recs) {
    writeBigsize(ty, out);
    const value = ty === assetTlv.PREV_WITNESS ? newPw : Array.from(v);
    writeBigsize(value.length, out);
    out.push(...value);
  }
  return new Uint8Array(out);
}

function u64beBig(n: bigint): Uint8Array {
  const b = new Uint8Array(8);
  for (let i = 7; i >= 0; i--) { b[i] = Number(n & 0xffn); n >>= 8n; }
  return b;
}

function tapBranchHash(a: Uint8Array, b: Uint8Array): Uint8Array {
  // BIP341 TapBranch: tagged hash over the two children in lexicographic order.
  let lo = a, hi = b;
  for (let i = 0; i < 32; i++) {
    if (a[i] !== b[i]) { if (a[i] > b[i]) { lo = b; hi = a; } break; }
  }
  const msg = new Uint8Array(64);
  msg.set(lo, 0); msg.set(hi, 32);
  return taggedHash("TapBranch", msg);
}

function siblingTapHash(value: Uint8Array): Uint8Array {
  if (value.length < 1) throw new ProofError("BadTapSibling");
  const ty = value[0];
  const preimage = value.subarray(1);
  if (ty === 0) return taggedHash("TapLeaf", preimage);
  if (ty === 1) {
    if (preimage.length !== 64) throw new ProofError("BadTapSibling");
    return tapBranchHash(preimage.subarray(0, 32), preimage.subarray(32));
  }
  throw new ProofError("BadTapSibling");
}

/** Reconstruct the on-chain x-only taproot output key the asset must be committed
 *  under, from the asset leaf + its CommitmentProof (DL-23). asset→AssetCommitment
 *  →TapCommitment MS-SMTs, the V2 tapscript leaf, optional sibling, then the
 *  taproot tweak. Returns 32 bytes. */
export function reconstructOutputKey(
  asset: AssetLeaf,
  assetLeafBytes: Uint8Array,
  cp: CommitmentProof,
  taproot: TaprootProof,
): Uint8Array {
  if (taproot.internalKey.length !== 33) throw new ProofError("BadFieldLength");
  if (cp.assetIdKey.length !== 32) throw new ProofError("BadFieldLength");
  if (asset.scriptKey.length !== 33) throw new ProofError("BadFieldLength");
  const assetIdKey = cp.assetIdKey;
  const xonlyScriptKey = asset.scriptKey.subarray(1, 33);

  // Inner AssetCommitment key: grouped ⇒ sha256(genesis_asset_id || xonly(sk));
  // ungrouped ⇒ sha256(xonly(sk)).
  const ack = asset.groupKey
    ? sha256(assetId(asset.genesis), xonlyScriptKey)
    : sha256(xonlyScriptKey);

  const stripped = splitCommittedLeafBytes(assetLeafBytes);
  const leafValue = stripped ?? assetLeafBytes;
  const innerSib = parseCompressedProof(cp.innerMssmt);
  if (!innerSib) throw new ProofError("BadFieldLength");
  const inner = proofRoot(ack, mssmtLeaf(leafValue, asset.amount), innerSib);

  // AssetCommitment.Root() = sha256(asset_id || rootL || rootR || u64_be(sum)).
  const acRoot = sha256(assetIdKey, inner.l, inner.r, u64beBig(inner.root.sum));

  // Outer MS-SMT: AssetCommitment in the TapCommitment, keyed by asset_id.
  // Leaf value = [ac_version] || ac.Root() || u64_be(sum).
  const tclValue = new Uint8Array(1 + 32 + 8);
  tclValue[0] = cp.assetProofVersion;
  tclValue.set(acRoot, 1);
  tclValue.set(u64beBig(inner.root.sum), 33);
  const outerSib = parseCompressedProof(cp.outerMssmt);
  if (!outerSib) throw new ProofError("BadFieldLength");
  const outer = proofRoot(assetIdKey, mssmtLeaf(tclValue, inner.root.sum), outerSib);

  // V2 tapscript leaf: tag || version || rootHash || u64_be(rootSum).
  if (cp.tapVersion !== 2) throw new ProofError("UnsupportedTapVersion");
  const tag = nobleSha256(ascii("taproot-assets:194243")) as Uint8Array;
  const script = new Uint8Array(32 + 1 + 32 + 8);
  script.set(tag, 0);
  script[32] = 2;
  script.set(outer.root.hash, 33);
  script.set(u64beBig(outer.root.sum), 65);
  const preimage = new Uint8Array(2 + script.length);
  preimage[0] = 0xc0;
  preimage[1] = script.length; // 73 < 0xfd
  preimage.set(script, 2);
  const taLeafHash = taggedHash("TapLeaf", preimage);

  const tapscriptRoot = cp.tapSibling
    ? tapBranchHash(taLeafHash, siblingTapHash(cp.tapSibling))
    : taLeafHash;

  // Taproot output key = taptweak(internal_xonly, tapscript_root).
  const internalXonlyHex = bytesToHex(taproot.internalKey.subarray(1, 33));
  return hexToBytes(tapTweakDataHex(internalXonlyHex, tapscriptRoot));
}

/** Verify a proof step is anchored AND its asset is committed in that anchor
 *  output (DL-23): the reconstructed taproot output key must equal the on-chain
 *  anchored output key. Returns the verified AnchorInfo. */
export function verifyAssetInclusion(p: Proof): AnchorInfo {
  const anchor = verifyAnchor(p);
  const leafBytes = proofGet(p, tlv.ASSET_LEAF);
  if (!leafBytes) throw new ProofError("Truncated");
  const asset = parseAssetLeaf(leafBytes);
  const inc = proofGet(p, tlv.INCLUSION_PROOF);
  if (!inc) throw new ProofError("Truncated");
  const taproot = parseTaprootProof(inc);
  if (!taproot.commitmentProof) throw new ProofError("Truncated");
  const cp = parseCommitmentProof(taproot.commitmentProof);
  const key = reconstructOutputKey(asset, leafBytes, cp, taproot);
  if (!eqBytes(key, anchor.taprootOutputKey)) throw new ProofError("CommitmentMismatch");
  return anchor;
}

function prevOutpoint(p: Proof): { txid: Uint8Array; vout: number } {
  const v = proofGet(p, tlv.PREV_OUT);
  if (!v) throw new ProofError("Truncated");
  if (v.length !== 36) throw new ProofError("BadFieldLength");
  return { txid: v.subarray(0, 32), vout: ((v[32] << 24) | (v[33] << 16) | (v[34] << 8) | v[35]) >>> 0 };
}

/** Verify a proof file's full lineage: every step's asset is committed in its
 *  anchor output, and each step's input is the previous step's anchored output —
 *  a connected chain from genesis to the latest state. Returns the head AnchorInfo. */
export function verifyLineage(proofs: Proof[]): AnchorInfo {
  let prev: AnchorInfo | null = null;
  for (const p of proofs) {
    const anchor = verifyAssetInclusion(p);
    if (prev) {
      const { txid, vout } = prevOutpoint(p);
      if (!eqBytes(txid, prev.txid) || vout !== prev.outputIndex) {
        throw new ProofError("LineageBroken");
      }
    }
    prev = anchor;
  }
  if (!prev) throw new ProofError("EmptyLineage");
  return prev;
}
