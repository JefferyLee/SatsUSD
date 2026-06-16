// Minimal MS-SMT (Merkle Sum Sparse Merkle Tree) verification for tapd
// commitment proofs (tapd v0.7.2 mssmt). Port of crates/satusd-ta-proof/src/mssmt.rs.
//
// Node hashing (single SHA-256, no tagging):
//   leaf:   sha256( value || u64_be(sum) )
//   branch: sha256( left_hash || right_hash || u64_be(left_sum + right_sum) )
//   empty leaf: value=∅, sum=0 ⇒ sha256( u64_be(0) ).
// Depth 256; the all-empty subtree root at each level is precomputed.
import { sha256 as nobleSha256 } from "@noble/hashes/sha2.js";

export const TREE_LEVELS = 256;

export interface Node {
  hash: Uint8Array; // 32
  sum: bigint; // u64
}

function u64be(n: bigint): Uint8Array {
  const b = new Uint8Array(8);
  for (let i = 7; i >= 0; i--) { b[i] = Number(n & 0xffn); n >>= 8n; }
  return b;
}
function cat(...parts: Uint8Array[]): Uint8Array {
  const len = parts.reduce((a, p) => a + p.length, 0);
  const out = new Uint8Array(len);
  let o = 0;
  for (const p of parts) { out.set(p, o); o += p.length; }
  return out;
}

const sha = (b: Uint8Array): Uint8Array => nobleSha256(b) as Uint8Array;

export function leaf(value: Uint8Array, sum: bigint): Node {
  return { hash: sha(cat(value, u64be(sum))), sum };
}
export function branch(l: Node, r: Node): Node {
  const sum = l.sum + r.sum;
  return { hash: sha(cat(l.hash, r.hash, u64be(sum))), sum };
}

/** empty[i] = root of an all-empty subtree spanning levels i..=256;
 *  empty[256] = empty leaf; empty[i] = branch(empty[i+1], empty[i+1]). */
function emptyTree(): Node[] {
  const e: Node[] = new Array(TREE_LEVELS + 1);
  e[TREE_LEVELS] = leaf(new Uint8Array(0), 0n);
  for (let i = TREE_LEVELS - 1; i >= 0; i--) e[i] = branch(e[i + 1], e[i + 1]);
  return e;
}
const EMPTY = emptyTree();

/** tapd bitIndex(idx, key) = (key[idx/8] >> (idx%8)) & 1 (LSB-first within byte). */
function keyBit(key: Uint8Array, idx: number): boolean {
  return ((key[idx >> 3] >> (idx % 8)) & 1) === 1;
}

export interface InclusionProof {
  siblings: Node[]; // 256, leaf-first (siblings[0] = sibling at the leaf level)
}

/** Decode a tapd mssmt.CompressedProof:
 *  u16 numNodes(BE) || numNodes×( hash[32] || u64 sum(BE) ) || bits[32],
 *  then decompress (set bit ⇒ empty subtree root at that level; clear ⇒ next node). */
export function parseCompressedProof(bytes: Uint8Array): InclusionProof | null {
  if (bytes.length < 2) return null;
  const numNodes = (bytes[0] << 8) | bytes[1];
  let off = 2;
  const explicit: Node[] = [];
  for (let k = 0; k < numNodes; k++) {
    if (off + 40 > bytes.length) return null;
    const hash = bytes.slice(off, off + 32);
    let sum = 0n;
    for (let j = 0; j < 8; j++) sum = (sum << 8n) | BigInt(bytes[off + 32 + j]);
    explicit.push({ hash, sum });
    off += 40;
  }
  if (off + 32 !== bytes.length) return null;
  const bits = bytes.subarray(off, off + 32);
  const siblings: Node[] = new Array(TREE_LEVELS);
  let next = 0;
  for (let i = 0; i < TREE_LEVELS; i++) {
    const bitSet = ((bits[i >> 3] >> (i % 8)) & 1) === 1;
    if (bitSet) {
      siblings[i] = EMPTY[TREE_LEVELS - i];
    } else {
      if (next >= explicit.length) return null;
      siblings[i] = explicit[next++];
    }
  }
  if (next !== explicit.length) return null;
  return { siblings };
}

/** Fold a leaf up to the MS-SMT root through a decompressed proof's siblings —
 *  tapd walkUp. bit(key,i)==0 ⇒ current is the left child. Returns the root plus
 *  its two top children's hashes (needed by AssetCommitment.Root()). */
export function proofRoot(
  key: Uint8Array,
  leafNode: Node,
  p: InclusionProof,
): { root: Node; l: Uint8Array; r: Uint8Array } {
  let cur = leafNode;
  let topL = leafNode.hash; // placeholder; always overwritten at i === 0
  let topR = leafNode.hash;
  for (let i = TREE_LEVELS - 1; i >= 0; i--) {
    const sib = p.siblings[TREE_LEVELS - 1 - i];
    const [l, r] = keyBit(key, i) ? [sib, cur] : [cur, sib];
    if (i === 0) { topL = l.hash; topR = r.hash; }
    cur = branch(l, r);
  }
  return { root: cur, l: topL, r: topR };
}
