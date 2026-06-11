//! Spec 00 §3 encoding conventions: big-endian integers, BigSize
//! length prefixes, varbytes, and BIP-340 tagged hashes under the
//! `SatUSD/<purpose>/v<N>` namespace.

use sha2::{Digest, Sha256};

/// BIP-340 tagged hash: `SHA256(SHA256(tag) || SHA256(tag) || msg)`.
pub fn tagged_hash(tag: &str, msg: &[u8]) -> [u8; 32] {
    let tag_digest = Sha256::digest(tag.as_bytes());
    let mut h = Sha256::new();
    h.update(tag_digest);
    h.update(tag_digest);
    h.update(msg);
    h.finalize().into()
}

/// Canonical-encoding sink. All `put_*` writers follow spec 00 §3.
#[derive(Default)]
pub struct Encoder {
    buf: Vec<u8>,
}

impl Encoder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.buf
    }

    pub fn put_u8(&mut self, v: u8) {
        self.buf.push(v);
    }

    pub fn put_bool(&mut self, v: bool) {
        self.buf.push(u8::from(v));
    }

    pub fn put_u16(&mut self, v: u16) {
        self.buf.extend_from_slice(&v.to_be_bytes());
    }

    pub fn put_u32(&mut self, v: u32) {
        self.buf.extend_from_slice(&v.to_be_bytes());
    }

    pub fn put_u64(&mut self, v: u64) {
        self.buf.extend_from_slice(&v.to_be_bytes());
    }

    pub fn put_i32(&mut self, v: i32) {
        self.buf.extend_from_slice(&v.to_be_bytes());
    }

    pub fn put_bytes32(&mut self, v: &[u8; 32]) {
        self.buf.extend_from_slice(v);
    }

    /// BigSize per dlcspecs/BOLT: 1, 3, 5, or 9 bytes, big-endian.
    pub fn put_bigsize(&mut self, v: u64) {
        match v {
            0..=0xfc => self.buf.push(v as u8),
            0xfd..=0xffff => {
                self.buf.push(0xfd);
                self.buf.extend_from_slice(&(v as u16).to_be_bytes());
            }
            0x1_0000..=0xffff_ffff => {
                self.buf.push(0xfe);
                self.buf.extend_from_slice(&(v as u32).to_be_bytes());
            }
            _ => {
                self.buf.push(0xff);
                self.buf.extend_from_slice(&v.to_be_bytes());
            }
        }
    }

    /// Raw bytes, no prefix — for pre-encoded substructures.
    pub fn put_raw(&mut self, v: &[u8]) {
        self.buf.extend_from_slice(v);
    }

    /// BigSize length prefix followed by the raw bytes.
    pub fn put_varbytes(&mut self, v: &[u8]) {
        self.put_bigsize(v.len() as u64);
        self.buf.extend_from_slice(v);
    }

    /// UTF-8 string as varbytes.
    pub fn put_string(&mut self, v: &str) {
        self.put_varbytes(v.as_bytes());
    }
}

/// Strict reader for spec 00 §3 encodings — the decode mirror of
/// `Encoder`. Every read is bounds-checked; callers assert
/// exhaustion via `done()`.
pub struct Decoder<'a> {
    buf: &'a [u8],
    pos: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecodeError {
    /// Read past the end of the buffer.
    Truncated,
    /// BigSize not minimally encoded (BOLT requirement).
    NonCanonicalBigSize,
    /// Varbytes declared a length that cannot fit.
    Oversized,
    /// String bytes are not UTF-8.
    BadUtf8,
}

impl<'a> Decoder<'a> {
    pub fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    pub fn done(&self) -> bool {
        self.pos == self.buf.len()
    }

    pub fn remaining(&self) -> usize {
        self.buf.len() - self.pos
    }

    pub fn take(&mut self, n: usize) -> Result<&'a [u8], DecodeError> {
        if self.remaining() < n {
            return Err(DecodeError::Truncated);
        }
        let out = &self.buf[self.pos..self.pos + n];
        self.pos += n;
        Ok(out)
    }

    pub fn u16(&mut self) -> Result<u16, DecodeError> {
        Ok(u16::from_be_bytes(self.take(2)?.try_into().unwrap()))
    }

    pub fn u32(&mut self) -> Result<u32, DecodeError> {
        Ok(u32::from_be_bytes(self.take(4)?.try_into().unwrap()))
    }

    pub fn bytes32(&mut self) -> Result<[u8; 32], DecodeError> {
        Ok(self.take(32)?.try_into().unwrap())
    }

    pub fn bigsize(&mut self) -> Result<u64, DecodeError> {
        let first = self.take(1)?[0];
        let v = match first {
            0..=0xfc => u64::from(first),
            0xfd => u64::from(u16::from_be_bytes(self.take(2)?.try_into().unwrap())),
            0xfe => u64::from(u32::from_be_bytes(self.take(4)?.try_into().unwrap())),
            0xff => u64::from_be_bytes(self.take(8)?.try_into().unwrap()),
        };
        let minimal = match first {
            0..=0xfc => true,
            0xfd => v >= 0xfd,
            0xfe => v > 0xffff,
            0xff => v > 0xffff_ffff,
        };
        if !minimal {
            return Err(DecodeError::NonCanonicalBigSize);
        }
        Ok(v)
    }

    pub fn varbytes(&mut self) -> Result<&'a [u8], DecodeError> {
        let len = self.bigsize()?;
        let len = usize::try_from(len).map_err(|_| DecodeError::Oversized)?;
        self.take(len)
    }

    pub fn string(&mut self) -> Result<&'a str, DecodeError> {
        std::str::from_utf8(self.varbytes()?).map_err(|_| DecodeError::BadUtf8)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bigsize_boundaries() {
        let cases: [(u64, &[u8]); 7] = [
            (0, &[0x00]),
            (0xfc, &[0xfc]),
            (0xfd, &[0xfd, 0x00, 0xfd]),
            (0xffff, &[0xfd, 0xff, 0xff]),
            (0x1_0000, &[0xfe, 0x00, 0x01, 0x00, 0x00]),
            (0xffff_ffff, &[0xfe, 0xff, 0xff, 0xff, 0xff]),
            (
                0x1_0000_0000,
                &[0xff, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00],
            ),
        ];
        for (v, want) in cases {
            let mut e = Encoder::new();
            e.put_bigsize(v);
            assert_eq!(e.into_bytes(), want, "bigsize({v})");
        }
    }

    #[test]
    fn integers_are_big_endian() {
        let mut e = Encoder::new();
        e.put_u16(0x0102);
        e.put_u32(0x0304_0506);
        e.put_u64(0x0708_090a_0b0c_0d0e);
        e.put_i32(-2);
        assert_eq!(
            e.into_bytes(),
            [
                0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e,
                0xff, 0xff, 0xff, 0xfe
            ]
        );
    }

    #[test]
    fn decoder_roundtrips_and_rejects() {
        let mut e = Encoder::new();
        e.put_u16(0x0102);
        e.put_bigsize(0xfd);
        e.put_varbytes(b"abc");
        e.put_string("xy");
        let bytes = e.into_bytes();

        let mut d = Decoder::new(&bytes);
        assert_eq!(d.u16().unwrap(), 0x0102);
        assert_eq!(d.bigsize().unwrap(), 0xfd);
        assert_eq!(d.varbytes().unwrap(), b"abc");
        assert_eq!(d.string().unwrap(), "xy");
        assert!(d.done());

        // Truncation.
        let mut d = Decoder::new(&bytes[..1]);
        assert_eq!(d.u16(), Err(DecodeError::Truncated));

        // Non-minimal bigsize: 0xfc encoded with the 3-byte form.
        let mut d = Decoder::new(&[0xfd, 0x00, 0xfc]);
        assert_eq!(d.bigsize(), Err(DecodeError::NonCanonicalBigSize));

        // Varbytes longer than the buffer.
        let mut d = Decoder::new(&[0x05, 0x01]);
        assert_eq!(d.varbytes(), Err(DecodeError::Truncated));
    }

    #[test]
    fn tagged_hash_matches_bip340_shape() {
        // Independently recompute the construction.
        let tag = "SatUSD/test/v1";
        let msg = b"abc";
        let td = Sha256::digest(tag.as_bytes());
        let mut h = Sha256::new();
        h.update(td);
        h.update(td);
        h.update(msg);
        let want: [u8; 32] = h.finalize().into();
        assert_eq!(tagged_hash(tag, msg), want);
    }
}
