//! Deterministic byte source for reproducible fixtures (no RNG crate needed).
//! Field values are emitted into the JSON, so only Rust generates; TypeScript
//! reads the values back.

use sha2::{Digest, Sha256};

pub struct Det {
    ctr: u64,
}

impl Det {
    pub fn new(seed: u64) -> Self {
        Det { ctr: seed }
    }

    pub fn bytes(&mut self, n: usize) -> Vec<u8> {
        let mut out = Vec::with_capacity(n);
        while out.len() < n {
            let mut h = Sha256::new();
            h.update(b"satusd-det");
            h.update(self.ctr.to_be_bytes());
            out.extend_from_slice(&h.finalize());
            self.ctr += 1;
        }
        out.truncate(n);
        out
    }

    pub fn arr<const N: usize>(&mut self) -> [u8; N] {
        let v = self.bytes(N);
        let mut a = [0u8; N];
        a.copy_from_slice(&v);
        a
    }

    pub fn u64(&mut self) -> u64 {
        u64::from_be_bytes(self.arr::<8>())
    }

    pub fn u32(&mut self) -> u32 {
        u32::from_be_bytes(self.arr::<4>())
    }

    pub fn u16(&mut self) -> u16 {
        u16::from_be_bytes(self.arr::<2>())
    }

    pub fn u8(&mut self) -> u8 {
        self.bytes(1)[0]
    }

    /// Pick how many elements a variable-length field gets (0..=max).
    pub fn count(&mut self, max: usize) -> usize {
        (self.u8() as usize) % (max + 1)
    }
}
