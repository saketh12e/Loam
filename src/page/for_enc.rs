//! Frame-of-reference (FOR) pages: values stored as offsets from a frame
//! minimum at a fixed bit width, sharing the bitpack machinery.

use crate::bytes::{write_varint, Reader};
use crate::error::Result;
use crate::page::bitpack;

/// Decode a FOR page: a frame minimum plus packed offsets.
pub fn parse(payload: &[u8], rows: u32) -> Result<Vec<i64>> {
    let mut r = Reader::new(payload);
    let frame = crate::bytes::unzigzag(r.varint("for frame")? as u64);
    let width = r.u8("for width")?;
    let offsets = bitpack::parse(r.take(r.remaining(), "for offsets")?, rows, width)?;
    Ok(offsets.iter().map(|&o| frame + o as i64).collect())
}

/// Encode values as a FOR payload. Width is the minimum bit width covering
/// the max offset.
pub fn write(values: &[i64]) -> Vec<u8> {
    let frame = values.iter().copied().min().unwrap_or(0);
    let max_off = values.iter().map(|v| v - frame).max().unwrap_or(0) as u64;
    let width = (64 - max_off.leading_zeros()).max(1) as u8;
    let mut out = Vec::new();
    write_varint(&mut out, crate::bytes::zigzag(frame) as u32);
    out.push(width);
    let offsets: Vec<u64> = values.iter().map(|v| (v - frame) as u64).collect();
    out.extend_from_slice(&bitpack::write(&offsets, width));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_tight_range() {
        let values = vec![1000i64, 1003, 1001, 1017, 1002];
        let payload = write(&values);
        assert_eq!(parse(&payload, values.len() as u32).unwrap(), values);
    }

    #[test]
    fn round_trip_negative_frame() {
        let values = vec![-50i64, -49, -60, -51];
        let payload = write(&values);
        assert_eq!(parse(&payload, values.len() as u32).unwrap(), values);
    }

    #[test]
    fn rejects_truncated() {
        let payload = write(&[1i64, 2, 3]);
        assert!(parse(&payload[..payload.len() - 2], 3).is_err());
    }
}
