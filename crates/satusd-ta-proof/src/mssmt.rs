//! Minimal MS-SMT (Merkle Sum Sparse Merkle Tree) verification for tapd
//! commitment proofs (tapd v0.7.2 `mssmt`).
//!
//! Node hashing (confirmed against `mssmt/node.go`), single SHA-256, no tagging:
//! - leaf:   `sha256( value || u64_be(sum) )`
//! - branch: `sha256( left_hash || right_hash || u64_be(left_sum + right_sum) )`
//! - empty leaf: `value = ∅`, `sum = 0` ⇒ `sha256( u64_be(0) )`.
//!
//! Depth is 256 levels; the all-empty subtree root at each level is precomputed.
//!
//! [`parse_compressed_proof`] decodes tapd's `CompressedProof` wire format and
//! [`proof_root`] folds a leaf up through the proof's siblings (tapd `walkUp`),
//! so both single-leaf (mint) and multi-leaf (transfer) commitments are handled.
//! The fold direction + hashing are validated byte-for-byte against live tapd via
//! the end-to-end output-key reconstruction in `lib.rs`.

use sha2::{Digest, Sha256};

pub const TREE_LEVELS: usize = 256;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Node {
    pub hash: [u8; 32],
    pub sum: u64,
}

/// `leaf = sha256(value || u64_be(sum))`.
pub fn leaf(value: &[u8], sum: u64) -> Node {
    let mut h = Sha256::new();
    h.update(value);
    h.update(sum.to_be_bytes());
    Node {
        hash: h.finalize().into(),
        sum,
    }
}

/// `branch = sha256(left.hash || right.hash || u64_be(left.sum + right.sum))`.
pub fn branch(l: &Node, r: &Node) -> Node {
    let sum = l.sum.checked_add(r.sum).expect("mssmt sum overflow");
    let mut h = Sha256::new();
    h.update(l.hash);
    h.update(r.hash);
    h.update(sum.to_be_bytes());
    Node {
        hash: h.finalize().into(),
        sum,
    }
}

/// `empty[i]` = root of an all-empty subtree spanning levels `i..=TREE_LEVELS`.
/// `empty[TREE_LEVELS]` is the empty leaf; `empty[i] = branch(empty[i+1], empty[i+1])`.
pub fn empty_tree() -> [Node; TREE_LEVELS + 1] {
    let mut e = [Node {
        hash: [0; 32],
        sum: 0,
    }; TREE_LEVELS + 1];
    e[TREE_LEVELS] = leaf(&[], 0);
    let mut i = TREE_LEVELS;
    while i > 0 {
        i -= 1;
        let child = e[i + 1];
        e[i] = branch(&child, &child);
    }
    e
}

/// tapd `bitIndex(idx, key) = (key[idx/8] >> (idx%8)) & 1` (LSB-first within byte).
fn key_bit(key: &[u8; 32], idx: usize) -> bool {
    (key[idx / 8] >> (idx % 8)) & 1 == 1
}

/// A decompressed MS-SMT inclusion proof: the 256 sibling nodes on the path from
/// the leaf to the root, **leaf-first** (`siblings[0]` is the sibling at the leaf
/// level, `siblings[255]` is the sibling just below the root) — the order tapd's
/// `CompressedProof.Decompress` produces.
pub struct InclusionProof {
    pub siblings: [Node; TREE_LEVELS],
}

/// Decode a tapd `mssmt.CompressedProof`:
/// `u16 numNodes(BE) || numNodes×( hash[32] || u64 sum(BE) ) || bits[32]`,
/// then decompress (set bit ⇒ that sibling is the empty subtree root at its
/// level; clear bit ⇒ take the next explicit node). Mirrors `Decompress`:
/// `nodes[i] = bit ? EmptyTree[256-i] : Nodes[next++]`.
pub fn parse_compressed_proof(bytes: &[u8]) -> Option<InclusionProof> {
    // u16 numNodes (big-endian).
    let num_nodes = u16::from_be_bytes(bytes.get(0..2)?.try_into().ok()?) as usize;
    let mut off = 2;
    let mut explicit = Vec::with_capacity(num_nodes);
    for _ in 0..num_nodes {
        let hash: [u8; 32] = bytes.get(off..off + 32)?.try_into().ok()?;
        let sum = u64::from_be_bytes(bytes.get(off + 32..off + 40)?.try_into().ok()?);
        explicit.push(Node { hash, sum });
        off += 40;
    }
    // 256-bit packed vector (32 bytes), LSB-first within each byte.
    let bits = bytes.get(off..off + 32)?;
    if off + 32 != bytes.len() {
        return None;
    }
    let empty = empty_tree();
    let mut siblings = [Node {
        hash: [0; 32],
        sum: 0,
    }; TREE_LEVELS];
    let mut next = 0;
    for (i, sib) in siblings.iter_mut().enumerate() {
        let bit_set = (bits[i / 8] >> (i % 8)) & 1 == 1;
        *sib = if bit_set {
            empty[TREE_LEVELS - i]
        } else {
            let n = *explicit.get(next)?;
            next += 1;
            n
        };
    }
    // A canonical proof uses exactly the explicit nodes the bits called for.
    if next != explicit.len() {
        return None;
    }
    Some(InclusionProof { siblings })
}

/// Fold a leaf up to the MS-SMT root through a decompressed proof's siblings —
/// tapd's `walkUp`. `bit(key, i)==0` ⇒ current is the left child. Returns the
/// root node plus its two children's hashes (needed by `AssetCommitment.Root()`).
pub fn proof_root(
    key: &[u8; 32],
    leaf_node: Node,
    siblings: &InclusionProof,
) -> (Node, [u8; 32], [u8; 32]) {
    let mut cur = leaf_node;
    let (mut top_l, mut top_r) = ([0u8; 32], [0u8; 32]);
    // walkUp: for i = 255..0, sibling = siblings[255 - i].
    for i in (0..TREE_LEVELS).rev() {
        let sib = siblings.siblings[TREE_LEVELS - 1 - i];
        let (l, r) = if key_bit(key, i) {
            (sib, cur)
        } else {
            (cur, sib)
        };
        if i == 0 {
            top_l = l.hash;
            top_r = r.hash;
        }
        cur = branch(&l, &r);
    }
    (cur, top_l, top_r)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An all-empty inclusion proof: every sibling is the empty subtree root at
    /// its level (`numNodes = 0`, all 256 bits set) — what a single-leaf tree's
    /// compressed proof decompresses to.
    fn all_empty() -> InclusionProof {
        let empty = empty_tree();
        let mut siblings = [Node {
            hash: [0; 32],
            sum: 0,
        }; TREE_LEVELS];
        for (i, sib) in siblings.iter_mut().enumerate() {
            *sib = empty[TREE_LEVELS - i];
        }
        InclusionProof { siblings }
    }

    #[test]
    fn empty_leaf_hash_matches_tapd() {
        // tapd EmptyLeafNode = NewLeafNode(nil, 0) ⇒ sha256(8 zero bytes).
        assert_eq!(
            hex::encode(leaf(&[], 0).hash),
            "af5570f5a1810b7af78caf4bc70a660f0df51e42baf91d4de5b2328de0e83dfc"
        );
    }

    #[test]
    fn empty_tree_is_self_consistent() {
        let e = empty_tree();
        assert_eq!(e[TREE_LEVELS], leaf(&[], 0));
        for i in 0..TREE_LEVELS {
            assert_eq!(e[i], branch(&e[i + 1], &e[i + 1]));
            assert_eq!(e[i].sum, 0); // empty subtrees carry zero sum
        }
    }

    #[test]
    fn proof_root_carries_sum_and_differs_by_key() {
        // The root's sum is the leaf's sum; different keys ⇒ different roots.
        let l = leaf(b"asset", 1_000_000);
        let empty = all_empty();
        let (r0, _, _) = proof_root(&[0x00; 32], l, &empty);
        let (r1, _, _) = proof_root(&[0xff; 32], l, &empty);
        assert_eq!(r0.sum, 1_000_000);
        assert_eq!(r1.sum, 1_000_000);
        assert_ne!(r0.hash, r1.hash);
    }

    #[test]
    fn parse_compressed_proof_single_leaf_is_all_empty() {
        // A single-leaf tree's compressed proof is `numNodes=0` + all-bits-set;
        // it must decompress to the all-empty sibling path.
        let mut bytes = vec![0u8, 0u8]; // numNodes = 0
        bytes.extend_from_slice(&[0xff; 32]); // all 256 bits set
        let p = parse_compressed_proof(&bytes).expect("valid single-leaf proof");
        assert_eq!(p.siblings, all_empty().siblings);
    }
}
