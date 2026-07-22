//! Bounded byte readers and small encoding helpers shared by every container
//! the engine parses. All multi-byte integers are little-endian unless noted.

use crate::error::{Error, Result};

/// A cursor over a byte slice that refuses to read past its end.
#[derive(Debug, Clone)]
pub struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    pub fn new(buf: &'a [u8]) -> Self {
        Reader { buf, pos: 0 }
    }

    pub fn remaining(&self) -> usize {
        self.buf.len().saturating_sub(self.pos)
    }

    pub fn position(&self) -> usize {
        self.pos
    }

    pub fn is_empty(&self) -> bool {
        self.remaining() == 0
    }

    pub fn take(&mut self, n: usize, what: &'static str) -> Result<&'a [u8]> {
        if self.remaining() < n {
            return Err(Error::UnexpectedEof { what });
        }
        let out = &self.buf[self.pos..self.pos + n];
        self.pos += n;
        Ok(out)
    }

    pub fn skip(&mut self, n: usize, what: &'static str) -> Result<()> {
        self.take(n, what).map(|_| ())
    }

    pub fn u8(&mut self, what: &'static str) -> Result<u8> {
        Ok(self.take(1, what)?[0])
    }

    pub fn u16(&mut self, what: &'static str) -> Result<u16> {
        let b = self.take(2, what)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }

    pub fn u32(&mut self, what: &'static str) -> Result<u32> {
        let b = self.take(4, what)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    pub fn u64(&mut self, what: &'static str) -> Result<u64> {
        let b = self.take(8, what)?;
        Ok(u64::from_le_bytes([
            b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
        ]))
    }

    pub fn i64(&mut self, what: &'static str) -> Result<i64> {
        Ok(self.u64(what)? as i64)
    }

    /// Unsigned LEB128 varint, at most 10 bytes for u64.
    pub fn varint64(&mut self, what: &'static str) -> Result<u64> {
        let mut value: u64 = 0;
        let mut shift = 0u32;
        loop {
            let byte = self.u8(what)?;
            value |= ((byte & 0x7f) as u64) << shift;
            if byte & 0x80 == 0 {
                return Ok(value);
            }
            shift += 7;
            if shift >= 70 {
                return Err(Error::LengthMismatch { what });
            }
        }
    }

    /// Unsigned LEB128 varint, at most 5 bytes for u32.
    pub fn varint(&mut self, what: &'static str) -> Result<u32> {
        let mut value: u32 = 0;
        let mut shift = 0u32;
        loop {
            let byte = self.u8(what)?;
            value |= ((byte & 0x7f) as u32) << shift;
            if byte & 0x80 == 0 {
                return Ok(value);
            }
            shift += 7;
            if shift >= 35 {
                return Err(Error::LengthMismatch { what });
            }
        }
    }

    /// Length-prefixed slice: varint length followed by that many bytes.
    pub fn len_slice(&mut self, what: &'static str) -> Result<&'a [u8]> {
        let n = self.varint(what)? as usize;
        self.take(n, what)
    }
}

/// Append a varint to a byte vector.
pub fn write_varint(out: &mut Vec<u8>, mut v: u32) {
    loop {
        let mut byte = (v & 0x7f) as u8;
        v >>= 7;
        if v != 0 {
            byte |= 0x80;
        }
        out.push(byte);
        if v == 0 {
            break;
        }
    }
}

/// Append a 64-bit varint to a byte vector.
pub fn write_varint64(out: &mut Vec<u8>, mut v: u64) {
    loop {
        let mut byte = (v & 0x7f) as u8;
        v >>= 7;
        if v != 0 {
            byte |= 0x80;
        }
        out.push(byte);
        if v == 0 {
            break;
        }
    }
}

pub fn write_len_slice(out: &mut Vec<u8>, data: &[u8]) {
    write_varint(out, data.len() as u32);
    out.extend_from_slice(data);
}

/// CRC-32C (Castagnoli), table-driven. Used for page and footer checksums.
pub struct Crc32c {
    table: [u32; 256],
}

impl Default for Crc32c {
    fn default() -> Self {
        Self::new()
    }
}

impl Crc32c {
    pub const fn new() -> Self {
        let mut table = [0u32; 256];
        let poly: u32 = 0x82f6_3b78;
        let mut i = 0;
        while i < 256 {
            let mut crc = i as u32;
            let mut k = 0;
            while k < 8 {
                crc = if crc & 1 != 0 {
                    (crc >> 1) ^ poly
                } else {
                    crc >> 1
                };
                k += 1;
            }
            table[i] = crc;
            i += 1;
        }
        Crc32c { table }
    }

    pub fn sum(&self, data: &[u8]) -> u32 {
        let mut crc = !0u32;
        for &b in data {
            crc = self.table[((crc ^ b as u32) & 0xff) as usize] ^ (crc >> 8);
        }
        !crc
    }
}

/// Shared instance; the table is cheap but there is no reason to rebuild it.
pub fn crc32c(data: &[u8]) -> u32 {
    const CRC: Crc32c = Crc32c::new();
    CRC.sum(data)
}

/// Zig-zag encode for delta pages.
pub fn zigzag(v: i64) -> u64 {
    ((v << 1) ^ (v >> 63)) as u64
}

pub fn unzigzag(v: u64) -> i64 {
    ((v >> 1) as i64) ^ -((v & 1) as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reader_bounds_are_enforced() {
        let data = [1u8, 2, 3];
        let mut r = Reader::new(&data);
        assert_eq!(r.u8("t").unwrap(), 1);
        assert!(r.take(4, "t").is_err());
        assert_eq!(r.remaining(), 2);
    }

    #[test]
    fn varint_round_trip() {
        for v in [0u32, 1, 127, 128, 300, 65_535, 1 << 20, u32::MAX] {
            let mut buf = Vec::new();
            write_varint(&mut buf, v);
            let mut r = Reader::new(&buf);
            assert_eq!(r.varint("v").unwrap(), v);
        }
    }

    #[test]
    fn varint_rejects_overlong() {
        let buf = [0xffu8; 6];
        let mut r = Reader::new(&buf);
        assert!(r.varint("v").is_err());
    }

    #[test]
    fn crc_known_vector() {
        // CRC-32C of the ASCII string "123456789" is 0xE3069283.
        assert_eq!(crc32c(b"123456789"), 0xE306_9283);
    }

    #[test]
    fn zigzag_round_trip() {
        for v in [0i64, -1, 1, i64::MIN + 1, i64::MAX, -123_456] {
            assert_eq!(unzigzag(zigzag(v)), v);
        }
    }

    #[test]
    fn len_slice_round_trip() {
        let mut buf = Vec::new();
        write_len_slice(&mut buf, b"hello");
        write_len_slice(&mut buf, b"");
        let mut r = Reader::new(&buf);
        assert_eq!(r.len_slice("a").unwrap(), b"hello");
        assert_eq!(r.len_slice("b").unwrap(), b"");
        assert!(r.is_empty());
    }

    #[test]
    fn varint64_round_trip() {
        for v in [0u64, 1, 300, 1 << 32, u64::MAX] {
            let mut buf = Vec::new();
            write_varint64(&mut buf, v);
            let mut r = Reader::new(&buf);
            assert_eq!(r.varint64("v").unwrap(), v);
        }
    }

    #[test]
    fn reader_position_tracks() {
        let data = [9u8; 10];
        let mut r = Reader::new(&data);
        assert_eq!(r.position(), 0);
        r.u32("t").unwrap();
        assert_eq!(r.position(), 4);
        r.skip(6, "t").unwrap();
        assert!(r.is_empty());
    }
}
