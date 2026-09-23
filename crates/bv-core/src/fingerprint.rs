//! Byte-exact port of the Go v0.25.0 `fingerprintWriter` (pkg/analysis/cache.go:437-540).
//!
//! v0.25.0 replaced the v0.20.0 flat NUL-joined encoding with a length-prefixed,
//! type-tagged streaming encoder. The aggregate `data_hash` is now a full 64-char
//! SHA-256 hex digest (not the 16-char truncation v0.20.0 emitted).
//!
//! The writer streams through a fixed 256-byte buffer; this only batches writes.
//! Field bytes and SHA-256 are identical to hashing directly — matching Go.

use sha2::{Digest, Sha256};

const BUF_LEN: usize = 256;
/// Go `binary.MaxVarintLen64` — the reserve that forces a flush before a varint.
const MAX_VARINT_LEN_64: usize = 10;

/// Streaming, length-prefixed encoder matching Go's `fingerprintWriter`.
pub struct FingerprintWriter {
    hash: Sha256,
    buffer: [u8; BUF_LEN],
    used: usize,
}

impl Default for FingerprintWriter {
    fn default() -> Self {
        Self::new()
    }
}

impl FingerprintWriter {
    pub fn new() -> Self {
        Self {
            hash: Sha256::new(),
            buffer: [0u8; BUF_LEN],
            used: 0,
        }
    }

    /// Go `reset()` — clears the hash and the pending buffer span.
    pub fn reset(&mut self) {
        self.hash = Sha256::new();
        self.used = 0;
    }

    /// Go `flush()` — drain the pending buffer into the hash.
    fn flush(&mut self) {
        if self.used > 0 {
            let chunk: Vec<u8> = self.buffer[..self.used].to_vec();
            self.hash.update(&chunk);
            self.used = 0;
        }
    }

    /// Go `writeByte()`.
    fn write_byte(&mut self, v: u8) {
        if self.used == BUF_LEN {
            self.flush();
        }
        self.buffer[self.used] = v;
        self.used += 1;
    }

    /// Go `sumHex()` — full SHA-256 hex, 64 lowercase chars.
    /// Clones the hasher so the writer stays reusable, matching Go's
    /// `hash.Sum(nil)` which never consumes the running hash.
    pub fn sum_hex(&mut self) -> String {
        self.flush();
        let digest = self.hash.clone().finalize();
        let mut out = String::with_capacity(64);
        for b in digest.iter() {
            out.push(char::from_digit((b >> 4) as u32, 16).unwrap());
            out.push(char::from_digit((b & 0x0f) as u32, 16).unwrap());
        }
        out
    }

    /// Go `writeStringHash()` — uvarint byte-length prefix, then raw bytes.
    pub fn write_string_hash(&mut self, v: &str) {
        self.write_uint_hash(v.len() as u64);
        let bytes = v.as_bytes();
        let mut rest = bytes;
        while !rest.is_empty() {
            if self.used == BUF_LEN {
                self.flush();
            }
            let n = rest.len().min(BUF_LEN - self.used);
            self.buffer[self.used..self.used + n].copy_from_slice(&rest[..n]);
            self.used += n;
            rest = &rest[n..];
        }
    }

    /// Go `writeStringPtrHash()` — nil tag 0, non-nil tag 1 then the string.
    pub fn write_string_ptr_hash(&mut self, v: Option<&str>) {
        match v {
            None => self.write_byte(0),
            Some(s) => {
                self.write_byte(1);
                self.write_string_hash(s);
            }
        }
    }

    /// Go `writeIntHash()` / `writeInt64Hash()` — varint (zigzag) encoding.
    pub fn write_int_hash(&mut self, v: i64) {
        if BUF_LEN - self.used < MAX_VARINT_LEN_64 {
            self.flush();
        }
        let mut buf = [0u8; MAX_VARINT_LEN_64];
        let n = put_varint(&mut buf, v);
        self.buffer[self.used..self.used + n].copy_from_slice(&buf[..n]);
        self.used += n;
    }

    /// Go `writeIntPtrHash()` — nil tag 0, non-nil tag 1 then the int.
    pub fn write_int_ptr_hash(&mut self, v: Option<i64>) {
        match v {
            None => self.write_byte(0),
            Some(x) => {
                self.write_byte(1);
                self.write_int_hash(x);
            }
        }
    }

    /// Go `writeUintHash()` — uvarint encoding.
    pub fn write_uint_hash(&mut self, v: u64) {
        if BUF_LEN - self.used < MAX_VARINT_LEN_64 {
            self.flush();
        }
        let mut buf = [0u8; MAX_VARINT_LEN_64];
        let n = put_uvarint(&mut buf, v);
        self.buffer[self.used..self.used + n].copy_from_slice(&buf[..n]);
        self.used += n;
    }

    /// Go `writeTimeHash()` — zero time becomes the empty string; otherwise
    /// `t.UTC().Format(time.RFC3339Nano)`. Rust timestamps are already stored
    /// in that normalized form by the loader, so we normalize defensively.
    pub fn write_time_hash(&mut self, t: Option<&str>) {
        let formatted = match t {
            None => String::new(),
            Some(raw) => {
                if raw.is_empty() {
                    String::new()
                } else {
                    normalize_time(raw)
                }
            }
        };
        self.write_string_hash(&formatted);
    }

    /// Go `writeTimePtrHash()` — nil tag 0, non-nil tag 1 then the time.
    pub fn write_time_ptr_hash(&mut self, t: Option<&str>) {
        match t {
            None => self.write_byte(0),
            Some(_) => {
                self.write_byte(1);
                self.write_time_hash(t);
            }
        }
    }
}

/// Go `t.UTC().Format(time.RFC3339Nano)`. Renders in UTC with a `Z` suffix
/// (Go's RFC3339Nano layout uses "Z07:00", printing "Z" for UTC), so an input
/// like `...+00:00` is normalized to `...Z` — a raw pass-through would change
/// the hash bytes.
fn normalize_time(raw: &str) -> String {
    match raw.parse::<jiff::Timestamp>() {
        Ok(ts) => {
            // jiff prints the smallest-precision RFC3339 form; append the
            // explicit UTC designator Go emits.
            let s = ts.to_string();
            if let Some(stripped) = s.strip_suffix("Z") {
                format!("{stripped}Z")
            } else if let Some(idx) = s.rfind(['+']) {
                if s.len() - idx == 6 {
                    format!("{}Z", &s[..idx])
                } else {
                    s
                }
            } else {
                format!("{s}Z")
            }
        }
        Err(_) => raw.to_string(),
    }
}

/// Public wrapper so `data_hash` normalizes dependency timestamps identically.
pub fn normalize_time_public(raw: &str) -> String {
    normalize_time(raw)
}

/// Go `binary.PutUvarint` — LEB128, low 7 bits per byte, high bit = continuation.
fn put_uvarint(buf: &mut [u8], mut v: u64) -> usize {
    let mut i = 0;
    while v >= 0x80 {
        buf[i] = (v as u8) | 0x80;
        v >>= 7;
        i += 1;
    }
    buf[i] = v as u8;
    i + 1
}

/// Go `binary.PutVarint` — zigzag then uvarint.
fn put_varint(buf: &mut [u8], v: i64) -> usize {
    let zz = ((v << 1) ^ (v >> 63)) as u64;
    put_uvarint(buf, zz)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uvarint_matches_go_encoding() {
        // Go: binary.PutUvarint(0)=1 byte 0x00; 300 -> 0xac 0x02.
        let mut b = [0u8; 10];
        assert_eq!(put_uvarint(&mut b, 0), 1);
        assert_eq!(b[0], 0x00);
        assert_eq!(put_uvarint(&mut b, 300), 2);
        assert_eq!(b[0], 0xac);
        assert_eq!(b[1], 0x02);
    }

    #[test]
    fn varint_is_zigzag() {
        // Go: binary.PutVarint(-1) -> 0x01, (0)->0x00, (1)->0x02.
        let mut b = [0u8; 10];
        assert_eq!(put_varint(&mut b, -1), 1);
        assert_eq!(b[0], 0x01);
        assert_eq!(put_varint(&mut b, 0), 1);
        assert_eq!(b[0], 0x00);
        assert_eq!(put_varint(&mut b, 1), 1);
        assert_eq!(b[0], 0x02);
    }

    #[test]
    fn sum_hex_is_64_chars() {
        let mut w = FingerprintWriter::new();
        assert_eq!(w.sum_hex().len(), 64);
    }

    #[test]
    fn string_ptr_nil_vs_empty_differ() {
        let mut a = FingerprintWriter::new();
        a.write_string_ptr_hash(None);
        let mut b = FingerprintWriter::new();
        b.write_string_ptr_hash(Some(""));
        assert_ne!(a.sum_hex(), b.sum_hex());
    }
}
