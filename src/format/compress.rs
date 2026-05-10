//! Lightweight frame compression for page payloads on the wire.
//!
//! The compression frame is an optional wrapper (`LC` magic) around an
//! encoded payload: a varint stream of literal runs and back-references.
//! It is deliberately simple — decode speed beats ratio for scan-heavy
//! workloads. Uncompressed payloads pass through untouched.

use crate::bytes::{write_varint, Reader};
use crate::error::{Error, Result};

pub const FRAME_MAGIC: &[u8; 2] = b"LC";
const MAX_MATCH: usize = 64;

/// Compress a payload into a compression frame.
pub fn compress(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() / 2 + 16);
    out.extend_from_slice(FRAME_MAGIC);
    write_varint(&mut out, data.len() as u32);
    let mut i = 0usize;
    let mut literals: Vec<u8> = Vec::new();
    let flush = |out: &mut Vec<u8>, literals: &mut Vec<u8>| {
        if !literals.is_empty() {
            write_varint(out, literals.len() as u32);
            out.extend_from_slice(literals);
            literals.clear();
        }
    };
    while i < data.len() {
        // Greedy match search in a small window.
        let start = i.saturating_sub(256);
        let mut best_len = 0usize;
        let mut best_dist = 0usize;
        let mut j = start;
        while j < i {
            let mut l = 0usize;
            while i + l < data.len() && data[j + l] == data[i + l] && l < MAX_MATCH {
                l += 1;
            }
            if l > best_len {
                best_len = l;
                best_dist = i - j;
            }
            j += 1;
        }
        if best_len >= 4 {
            flush(&mut out, &mut literals);
            write_varint(&mut out, 0);
            write_varint(&mut out, best_dist as u32);
            write_varint(&mut out, best_len as u32);
            i += best_len;
        } else {
            literals.push(data[i]);
            i += 1;
        }
    }
    flush(&mut out, &mut literals);
    out
}

/// Decompress a compression frame back into the original payload.
pub fn decompress(raw: &[u8]) -> Result<Vec<u8>> {
    let mut r = Reader::new(raw);
    if r.take(2, "frame magic")? != FRAME_MAGIC {
        return Err(Error::BadMagic {
            what: "compression frame",
        });
    }
    let declared = r.varint("frame length")? as usize;
    if declared > 1 << 24 {
        return Err(Error::LengthMismatch {
            what: "frame length",
        });
    }
    let mut out: Vec<u8> = Vec::with_capacity(declared);
    while !r.is_empty() {
        let lit = r.varint("frame literal run")? as usize;
        if lit > 0 {
            if lit > declared {
                return Err(Error::LengthMismatch {
                    what: "frame literal run",
                });
            }
            let bytes = r.take(lit, "frame literals")?;
            out.extend_from_slice(bytes);
        } else {
            let dist = r.varint("frame distance")? as usize;
            let len = r.varint("frame match")? as usize;
            if dist == 0 || dist > out.len() || len == 0 || len > MAX_MATCH {
                return Err(Error::LengthMismatch {
                    what: "frame match",
                });
            }
            let from = out.len() - dist;
            for k in 0..len {
                let b = out[from + k];
                out.push(b);
            }
        }
    }
    if out.len() != declared {
        return Err(Error::LengthMismatch {
            what: "frame decoded length",
        });
    }
    Ok(out)
}

/// Ratio helper for writer-side decisions: compress only when it pays.
pub fn worth_compressing(data: &[u8]) -> bool {
    if data.len() < 64 {
        return false;
    }
    let frame = compress(data);
    frame.len() * 10 < data.len() * 9
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_repetitive() {
        let data = b"abcabcabcabcabcabcabcabc".to_vec();
        let frame = compress(&data);
        assert!(frame.len() < data.len());
        assert_eq!(decompress(&frame).unwrap(), data);
    }

    #[test]
    fn round_trip_randomish() {
        let data: Vec<u8> = (0..200).map(|i| ((i * 37) % 251) as u8).collect();
        assert_eq!(decompress(&compress(&data)).unwrap(), data);
    }

    #[test]
    fn empty_input() {
        let data: Vec<u8> = Vec::new();
        assert_eq!(decompress(&compress(&data)).unwrap(), data);
    }

    #[test]
    fn rejects_bad_magic() {
        assert!(decompress(b"XX\x00").is_err());
    }

    #[test]
    fn rejects_overshoot_distance() {
        // Hand-build a frame with a match before the output start.
        let mut raw = Vec::new();
        raw.extend_from_slice(FRAME_MAGIC);
        write_varint(&mut raw, 8);
        write_varint(&mut raw, 0);
        write_varint(&mut raw, 9);
        write_varint(&mut raw, 4);
        assert!(decompress(&raw).is_err());
    }
}
