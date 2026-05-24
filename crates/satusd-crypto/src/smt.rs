//! Sparse Merkle tree, height 256, over Poseidon (PRD §6.1; ADR-0015).
//!
//! Conventions frozen for the M0 reference (the M4b circuit must match):
//! - Height 256. A key is a 32-byte value; its path is the 256 bits MSB-first
//!   (depth 0 = most-significant bit of byte 0).
//! - Empty leaf = field zero. An empty subtree of height `h` has a precomputed
//!   root `E[h]`, with `E[0] = 0` and `E[h] = poseidon2(E[h-1], E[h-1])`.
//! - Occupied leaf = `poseidon2(hash_bytes(key), hash_bytes(value))`.
//! - Internal node = `poseidon2(left, right)`.
//! - Membership proof = 256 sibling node values, indexed by depth (0 = top).
//!   Non-membership of a key = an inclusion proof of the empty leaf at its path.

use std::collections::BTreeMap;

use ark_bn254::Fr;

use crate::poseidon::{fr_from_be_bytes, fr_to_be_bytes, hash_bytes, poseidon2};

pub const HEIGHT: usize = 256;

/// An occupied (key, leaf-hash) entry used while folding the sparse tree.
type Entry = ([u8; 32], Fr);

fn empty_roots() -> Vec<Fr> {
    let mut e = Vec::with_capacity(HEIGHT + 1);
    e.push(Fr::from(0u64));
    for h in 1..=HEIGHT {
        let prev = e[h - 1];
        e.push(poseidon2(prev, prev));
    }
    e
}

/// Occupied leaf hash `poseidon2(hash_bytes(key), hash_bytes(value))`.
pub fn leaf_hash(key: &[u8; 32], value: &[u8; 32]) -> Fr {
    poseidon2(hash_bytes(key), hash_bytes(value))
}

fn bit(key: &[u8; 32], depth: usize) -> u8 {
    (key[depth / 8] >> (7 - (depth % 8))) & 1
}

/// In-memory sparse Merkle tree. Absent keys map to the empty (zero) leaf.
pub struct SparseMerkleTree {
    leaves: BTreeMap<[u8; 32], Fr>,
    empty: Vec<Fr>,
}

impl Default for SparseMerkleTree {
    fn default() -> Self {
        Self::new()
    }
}

impl SparseMerkleTree {
    pub fn new() -> Self {
        SparseMerkleTree {
            leaves: BTreeMap::new(),
            empty: empty_roots(),
        }
    }

    pub fn contains(&self, key: &[u8; 32]) -> bool {
        self.leaves.contains_key(key)
    }

    /// Insert/overwrite a key with `value`. Returns the previous occupancy.
    pub fn insert(&mut self, key: [u8; 32], value: &[u8; 32]) -> bool {
        self.leaves.insert(key, leaf_hash(&key, value)).is_some()
    }

    pub fn root(&self) -> [u8; 32] {
        let entries: Vec<Entry> = self.leaves.iter().map(|(k, v)| (*k, *v)).collect();
        fr_to_be_bytes(&self.subtree(0, &entries))
    }

    fn subtree(&self, depth: usize, entries: &[Entry]) -> Fr {
        if entries.is_empty() {
            return self.empty[HEIGHT - depth];
        }
        if depth == HEIGHT {
            return entries[0].1;
        }
        let (left, right) = partition(entries, depth);
        poseidon2(
            self.subtree(depth + 1, &left),
            self.subtree(depth + 1, &right),
        )
    }

    /// 256 sibling node values (indexed by depth) proving the key's leaf.
    pub fn prove(&self, key: &[u8; 32]) -> Vec<[u8; 32]> {
        let entries: Vec<Entry> = self.leaves.iter().map(|(k, v)| (*k, *v)).collect();
        let mut siblings = vec![[0u8; 32]; HEIGHT];
        self.collect_siblings(0, key, &entries, &mut siblings);
        siblings
    }

    fn collect_siblings(
        &self,
        depth: usize,
        key: &[u8; 32],
        entries: &[Entry],
        out: &mut [[u8; 32]],
    ) {
        if depth == HEIGHT {
            return;
        }
        let (left, right) = partition(entries, depth);
        if bit(key, depth) == 0 {
            out[depth] = fr_to_be_bytes(&self.subtree(depth + 1, &right));
            self.collect_siblings(depth + 1, key, &left, out);
        } else {
            out[depth] = fr_to_be_bytes(&self.subtree(depth + 1, &left));
            self.collect_siblings(depth + 1, key, &right, out);
        }
    }
}

fn partition(entries: &[Entry], depth: usize) -> (Vec<Entry>, Vec<Entry>) {
    let mut left = Vec::new();
    let mut right = Vec::new();
    for e in entries {
        if bit(&e.0, depth) == 0 {
            left.push(*e);
        } else {
            right.push(*e);
        }
    }
    (left, right)
}

/// Fold a leaf up through the proof to a root, applying path bits.
fn fold_to_root(key: &[u8; 32], leaf: Fr, proof: &[[u8; 32]]) -> [u8; 32] {
    let mut cur = leaf;
    for depth in (0..HEIGHT).rev() {
        let sib = fr_from_be_bytes(&proof[depth]);
        cur = if bit(key, depth) == 0 {
            poseidon2(cur, sib)
        } else {
            poseidon2(sib, cur)
        };
    }
    fr_to_be_bytes(&cur)
}

/// Verify `key -> value` is a member of `root`.
pub fn verify_inclusion(
    root: &[u8; 32],
    key: &[u8; 32],
    value: &[u8; 32],
    proof: &[[u8; 32]],
) -> bool {
    proof.len() == HEIGHT && &fold_to_root(key, leaf_hash(key, value), proof) == root
}

/// Verify `key` is absent from `root` (its leaf is empty).
pub fn verify_exclusion(root: &[u8; 32], key: &[u8; 32], proof: &[[u8; 32]]) -> bool {
    proof.len() == HEIGHT && &fold_to_root(key, Fr::from(0u64), proof) == root
}

/// Root obtained by setting `key -> value` against an existing membership/
/// non-membership `proof` (same sibling path). Lets a transition verifier compute
/// the post-update root from a single leaf change without holding the whole tree.
pub fn root_after_update(key: &[u8; 32], value: &[u8; 32], proof: &[[u8; 32]]) -> [u8; 32] {
    debug_assert_eq!(proof.len(), HEIGHT);
    fold_to_root(key, leaf_hash(key, value), proof)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn k(b: u8) -> [u8; 32] {
        [b; 32]
    }

    #[test]
    fn empty_root_is_deterministic() {
        assert_eq!(
            SparseMerkleTree::new().root(),
            SparseMerkleTree::new().root()
        );
    }

    #[test]
    fn inclusion_and_exclusion_verify() {
        let mut t = SparseMerkleTree::new();
        let key = k(0xa1);
        let val = k(0x01);
        t.insert(key, &val);
        let root = t.root();

        let inc = t.prove(&key);
        assert!(verify_inclusion(&root, &key, &val, &inc));

        // A different, absent key proves exclusion against the same root.
        let other = k(0xb2);
        let exc = t.prove(&other);
        assert!(verify_exclusion(&root, &other, &exc));
    }

    #[test]
    fn wrong_value_fails_inclusion() {
        let mut t = SparseMerkleTree::new();
        let key = k(0x33);
        t.insert(key, &k(0x01));
        let root = t.root();
        let proof = t.prove(&key);
        assert!(!verify_inclusion(&root, &key, &k(0x02), &proof));
    }

    #[test]
    fn tampered_sibling_fails() {
        let mut t = SparseMerkleTree::new();
        let key = k(0x44);
        let val = k(0x07);
        t.insert(key, &val);
        let root = t.root();
        let mut proof = t.prove(&key);
        proof[200][0] ^= 0xff;
        assert!(!verify_inclusion(&root, &key, &val, &proof));
    }

    // Lock-consume / nullifier semantics (R-09 / D17): non-membership holds before
    // insert and fails after — a second finalize of the same lock cannot prove absence.
    #[test]
    fn nullifier_non_membership_then_membership() {
        let mut t = SparseMerkleTree::new();
        let nf = k(0x9c);

        let root0 = t.root();
        let pre = t.prove(&nf);
        assert!(verify_exclusion(&root0, &nf, &pre), "absent before insert");

        let val = k(0x01);
        t.insert(nf, &val);
        let root1 = t.root();

        let post = t.prove(&nf);
        assert!(
            verify_inclusion(&root1, &nf, &val, &post),
            "present after insert"
        );
        assert!(
            !verify_exclusion(&root1, &nf, &post),
            "cannot prove absence after insert"
        );
    }
}
