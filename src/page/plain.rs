//! Plain (uncompressed) page kernels for the engine's three value types.
//!
//! Plain pages store values back to back with no encoding: fixed-width for
//! i64/f64, length-prefixed for UTF8. These kernels reinterpret payload
//! bytes as typed values for the scan path.

use crate::bytes::{write_len_slice, Reader};
use crate::error::{Error, Result};

/// Decode a plain i64 page into values.
pub fn parse_i64(payload: &[u8], rows: u32) -> Result<Vec<i64>> {
    let need = rows as usize * 8;
    if payload.len() < need {
        return Err(Error::UnexpectedEof {
            what: "plain i64 page",
        });
    }
    Ok(payload[..need]
        .chunks_exact(8)
        .map(|c| i64::from_le_bytes(c.try_into().unwrap()))
        .collect())
}

/// Decode a plain f64 page into values.
pub fn parse_f64(payload: &[u8], rows: u32) -> Result<Vec<f64>> {
    let need = rows as usize * 8;
    if payload.len() < need {
        return Err(Error::UnexpectedEof {
            what: "plain f64 page",
        });
    }
    Ok(payload[..need]
        .chunks_exact(8)
        .map(|c| f64::from_le_bytes(c.try_into().unwrap()))
        .collect())
}

/// Decode a plain UTF8 page: varint length-prefixed strings in row order.
pub fn parse_utf8(payload: &[u8], rows: u32) -> Result<Vec<Vec<u8>>> {
    let mut r = Reader::new(payload);
    let mut out = Vec::with_capacity(rows as usize);
    for _ in 0..rows {
        out.push(r.len_slice("plain utf8 value")?.to_vec());
    }
    Ok(out)
}

/// Serialize i64 values as a plain page payload.
pub fn write_i64(values: &[i64]) -> Vec<u8> {
    let mut out = Vec::with_capacity(values.len() * 8);
    for v in values {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out
}

/// Serialize f64 values as a plain page payload.
pub fn write_f64(values: &[f64]) -> Vec<u8> {
    let mut out = Vec::with_capacity(values.len() * 8);
    for v in values {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out
}

/// Serialize UTF8 values as a plain page payload.
pub fn write_utf8(values: &[&[u8]]) -> Vec<u8> {
    let mut out = Vec::new();
    for v in values {
        write_len_slice(&mut out, v);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn i64_round_trip() {
        let values = vec![1i64, -5, 1 << 40];
        let payload = write_i64(&values);
        assert_eq!(parse_i64(&payload, 3).unwrap(), values);
    }

    #[test]
    fn f64_round_trip() {
        let values = vec![1.5f64, -2.25, 0.0];
        let payload = write_f64(&values);
        assert_eq!(parse_f64(&payload, 3).unwrap(), values);
    }

    #[test]
    fn utf8_round_trip() {
        let payload = write_utf8(&[b"north", b"", b"south-east"]);
        let got = parse_utf8(&payload, 3).unwrap();
        assert_eq!(
            got,
            vec![b"north".to_vec(), Vec::new(), b"south-east".to_vec()]
        );
    }

    #[test]
    fn rejects_short_i64() {
        assert!(parse_i64(&[0u8; 7], 1).is_err());
    }

    #[test]
    fn rejects_short_utf8() {
        let payload = write_utf8(&[b"abc"]);
        assert!(parse_utf8(&payload, 2).is_err());
    }
}
