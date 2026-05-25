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
//! NB the single-leaf fold *direction* (key-bit → left/right) follows tapd's
//! convention but is only validated end-to-end when the commitment chain closes
//! to the on-chain taproot output key (incr 4b-iii). The hashing rules above are
//! exact.

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

/// Root of a tree that contains exactly `leaf_node` at `key` and is empty
/// elsewhere — every sibling on the path is the empty subtree root at its level.
/// (The single-leaf case is what tapd's compressed proofs reduce to for SatUSD's
/// one-asset-per-commitment layout: `numNodes = 0`, all 256 bits set.)
///
/// Returns the root node plus its two children's hashes (the root branch's left
/// and right), which `AssetCommitment.Root()` needs. Fold/direction verified
/// byte-for-byte against live tapd.
pub fn single_leaf_root(key: &[u8; 32], leaf_node: Node) -> (Node, [u8; 32], [u8; 32]) {
    let empty = empty_tree();
    let mut cur = leaf_node;
    let (mut top_l, mut top_r) = ([0u8; 32], [0u8; 32]);
    // Fold from the bottom (idx 255) to the root (idx 0); sibling at idx is the
    // empty subtree root `empty[idx + 1]`; bit 1 ⇒ current is the right child.
    for idx in (0..TREE_LEVELS).rev() {
        let sib = empty[idx + 1];
        let (l, r) = if key_bit(key, idx) {
            (sib, cur)
        } else {
            (cur, sib)
        };
        if idx == 0 {
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
    fn single_leaf_root_carries_sum_and_differs_by_key() {
        // The root's sum is the leaf's sum; different keys ⇒ different roots.
        let l = leaf(b"asset", 1_000_000);
        let (r0, _, _) = single_leaf_root(&[0x00; 32], l);
        let (r1, _, _) = single_leaf_root(&[0xff; 32], l);
        assert_eq!(r0.sum, 1_000_000);
        assert_eq!(r1.sum, 1_000_000);
        assert_ne!(r0.hash, r1.hash);
    }
}
