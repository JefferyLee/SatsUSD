// Canonical encoding primitives (PRD §18.1) — TypeScript reference.
// Must produce byte-identical output to crates/satusd-types/src/encoding.rs.

export class Encoder {
  private buf: number[] = [];

  bytes(): Uint8Array {
    return Uint8Array.from(this.buf);
  }

  u8(v: number): void {
    this.buf.push(v & 0xff);
  }

  u16(v: number): void {
    this.buf.push((v >>> 8) & 0xff, v & 0xff);
  }

  u32(v: number): void {
    this.buf.push((v >>> 24) & 0xff, (v >>> 16) & 0xff, (v >>> 8) & 0xff, v & 0xff);
  }

  u64(v: bigint): void {
    for (let shift = 56n; shift >= 0n; shift -= 8n) {
      this.buf.push(Number((v >> shift) & 0xffn));
    }
  }

  bool(v: boolean): void {
    this.buf.push(v ? 0x01 : 0x00);
  }

  enumU8(v: number): void {
    this.buf.push(v & 0xff);
  }

  // Fixed-width raw bytes, no length prefix.
  fixed(b: Uint8Array): void {
    for (const x of b) this.buf.push(x);
  }

  // Variable-length: u32 BE length prefix, then bytes.
  varBytes(b: Uint8Array): void {
    this.u32(b.length);
    this.fixed(b);
  }

  // Optional: 0x00 none / 0x01 some, then value if present.
  opt<T>(v: T | null, f: (e: Encoder, inner: T) => void): void {
    if (v === null || v === undefined) {
      this.u8(0x00);
    } else {
      this.u8(0x01);
      f(this, v);
    }
  }

  // Sequence: u32 BE count prefix, then each element.
  seq<T>(items: T[], f: (e: Encoder, item: T) => void): void {
    this.u32(items.length);
    for (const item of items) f(this, item);
  }
}

export function hexToBytes(hex: string): Uint8Array {
  if (hex.length % 2 !== 0) throw new Error(`odd-length hex: ${hex}`);
  const out = new Uint8Array(hex.length / 2);
  for (let i = 0; i < out.length; i++) {
    out[i] = parseInt(hex.slice(i * 2, i * 2 + 2), 16);
  }
  return out;
}

export function bytesToHex(b: Uint8Array): string {
  let s = "";
  for (const x of b) s += x.toString(16).padStart(2, "0");
  return s;
}
