//! Canonical encoding primitives (PRD §18.1).
//!
//! Rules: integers big-endian; bool as u8 (0/1); enum as u8; Optional as a u8
//! presence flag (0x00 none, 0x01 some) followed by the value; variable-length
//! byte fields as a u32 big-endian length prefix followed by the bytes.
//! Two implementations producing different bytes for the same value is a bug.

pub struct Encoder {
    buf: Vec<u8>,
}

impl Encoder {
    pub fn new() -> Self {
        Encoder { buf: Vec::new() }
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.buf
    }

    pub fn u8(&mut self, v: u8) {
        self.buf.push(v);
    }

    pub fn u16(&mut self, v: u16) {
        self.buf.extend_from_slice(&v.to_be_bytes());
    }

    pub fn u32(&mut self, v: u32) {
        self.buf.extend_from_slice(&v.to_be_bytes());
    }

    pub fn u64(&mut self, v: u64) {
        self.buf.extend_from_slice(&v.to_be_bytes());
    }

    pub fn bool(&mut self, v: bool) {
        self.buf.push(if v { 0x01 } else { 0x00 });
    }

    /// Enum discriminant as a single byte.
    pub fn enum_u8(&mut self, v: u8) {
        self.buf.push(v);
    }

    /// Fixed-width raw bytes, no length prefix (e.g. [u8; 32], [u8; 33], [u8; 80]).
    pub fn fixed(&mut self, b: &[u8]) {
        self.buf.extend_from_slice(b);
    }

    /// Variable-length byte field: u32 BE length prefix, then bytes.
    pub fn var_bytes(&mut self, b: &[u8]) {
        self.u32(b.len() as u32);
        self.buf.extend_from_slice(b);
    }

    /// Optional field: presence flag (0x00 none / 0x01 some), then the value if present.
    pub fn opt<T, F: FnOnce(&mut Encoder, &T)>(&mut self, v: &Option<T>, f: F) {
        match v {
            None => self.u8(0x00),
            Some(inner) => {
                self.u8(0x01);
                f(self, inner);
            }
        }
    }

    /// Sequence with a u32 BE count prefix, then each element via `f`.
    pub fn seq<T, F: Fn(&mut Encoder, &T)>(&mut self, items: &[T], f: F) {
        self.u32(items.len() as u32);
        for item in items {
            f(self, item);
        }
    }
}

impl Default for Encoder {
    fn default() -> Self {
        Self::new()
    }
}

pub trait Encode {
    fn encode(&self, e: &mut Encoder);
}

pub fn canonical_encode<T: Encode>(v: &T) -> Vec<u8> {
    let mut e = Encoder::new();
    v.encode(&mut e);
    e.into_bytes()
}
