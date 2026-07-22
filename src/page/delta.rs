//! Delta-encoded i64 pages: a base value followed by zig-zag varint deltas.

use crate::bytes::{unzigzag, write_varint64, zigzag, Reader};
use crate::error::{Error, Result};

pub fn parse(payload: &[u8], rows: u32) -> Result<Vec<i64>> {
    let mut r = Reader::new(payload);
    let base = unzigzag(r.varint64("delta base")?);
    let mut out = Vec::with_capacity(rows as usize);
    let mut cur = base;
    out.push(cur);
    for _ in 1..rows {
        let d = unzigzag(r.varint64("delta step")?);
        cur = cur
            .checked_add(d)
            .ok_or(Error::LengthMismatch { what: "delta step" })?;
        out.push(cur);
    }
    Ok(out)
}

pub fn write(values: &[i64]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut prev = 0i64;
    for (i, v) in values.iter().enumerate() {
        if i == 0 {
            write_varint64(&mut out, zigzag(*v));
        } else {
            write_varint64(&mut out, zigzag(v.wrapping_sub(prev)));
        }
        prev = *v;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let values = vec![100i64, 103, 101, 120, -40];
        let payload = write(&values);
        assert_eq!(parse(&payload, values.len() as u32).unwrap(), values);
    }

    #[test]
    fn round_trip_large_magnitudes() {
        // Steps fit in i64 but exceed the u32 varint range.
        let values = vec![1i64 << 40, (1 << 40) + 5, (1 << 40) + 9, (1 << 40) + 2];
        let payload = write(&values);
        assert_eq!(parse(&payload, values.len() as u32).unwrap(), values);
    }

    #[test]
    fn rejects_truncated_stream() {
        let payload = write(&[1i64, 2, 3]);
        assert!(parse(&payload[..payload.len() - 1], 3).is_err());
    }
}
