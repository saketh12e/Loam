//! Bit-packed unsigned pages: values at a fixed bit width, LSB-first.

use crate::error::{Error, Result};

/// Unpack `rows` values of `width` bits from `payload`.
pub fn parse(payload: &[u8], rows: u32, width: u8) -> Result<Vec<u64>> {
    if width == 0 || width > 32 {
        return Err(Error::LengthMismatch {
            what: "bitpack width",
        });
    }
    let need = (rows as usize * width as usize).div_ceil(8);
    if payload.len() < need {
        return Err(Error::UnexpectedEof {
            what: "bitpack payload",
        });
    }
    let mut out = Vec::with_capacity(rows as usize);
    let mut bit_pos = 0usize;
    for _ in 0..rows {
        let mut v = 0u64;
        for b in 0..width as usize {
            let byte = payload[(bit_pos + b) / 8];
            let bit = (byte >> ((bit_pos + b) % 8)) & 1;
            v |= (bit as u64) << b;
        }
        out.push(v);
        bit_pos += width as usize;
    }
    Ok(out)
}

pub fn write(values: &[u64], width: u8) -> Vec<u8> {
    let mut out = vec![0u8; (values.len() * width as usize).div_ceil(8)];
    let mut bit_pos = 0usize;
    for &v in values {
        for b in 0..width as usize {
            let bit = ((v >> b) & 1) as u8;
            out[(bit_pos + b) / 8] |= bit << ((bit_pos + b) % 8);
        }
        bit_pos += width as usize;
    }
    out
}

