//! Hybrid dictionary/RLE pages: a small word table plus runs of word ids.
//! Values expand into fixed-stride stripes sized for the decode cache line
//! budget; stripe assembly lives in `batch::StripeBuilder`.

use crate::bytes::{write_len_slice, write_varint, Reader};
use crate::error::{Error, Result};

/// Fixed stride of one expanded stripe slot.
pub const STRIPE_STRIDE: u16 = 64;
/// Hard cap on rows produced by one hybrid page.
pub const MAX_STRIPE_ROWS: u32 = 4096;

#[derive(Debug, Clone)]
pub struct HybridPage {
    pub words: Vec<Vec<u8>>,
    pub runs: Vec<(u32, u32)>, // (word id, run length)
}

pub fn parse(payload: &[u8]) -> Result<HybridPage> {
    let mut r = Reader::new(payload);
    let words = r.varint("hybrid word count")? as usize;
    if words == 0 || words > 1024 {
        return Err(Error::LengthMismatch {
            what: "hybrid words",
        });
    }
    let mut word_list = Vec::with_capacity(words);
    for _ in 0..words {
        let w = r.len_slice("hybrid word")?;
        if w.len() > STRIPE_STRIDE as usize {
            return Err(Error::LengthMismatch {
                what: "hybrid word length",
            });
        }
        word_list.push(w.to_vec());
    }
    let runs = r.varint("hybrid run count")? as usize;
    if runs == 0 || runs > 2048 {
        return Err(Error::LengthMismatch {
            what: "hybrid runs",
        });
    }
    let mut run_list = Vec::with_capacity(runs);
    for _ in 0..runs {
        let id = r.varint("hybrid word id")?;
        if id as usize >= word_list.len() {
            return Err(Error::LengthMismatch {
                what: "hybrid word id",
            });
        }
        let len = r.varint("hybrid run length")?;
        if len == 0 {
            return Err(Error::LengthMismatch {
                what: "hybrid run length",
            });
        }
        run_list.push((id, len));
    }
    Ok(HybridPage {
        words: word_list,
        runs: run_list,
    })
}

pub fn write(words: &[&[u8]], runs: &[(u32, u32)]) -> Vec<u8> {
    let mut out = Vec::new();
    write_varint(&mut out, words.len() as u32);
    for w in words {
        write_len_slice(&mut out, w);
    }
    write_varint(&mut out, runs.len() as u32);
    for (id, len) in runs {
        write_varint(&mut out, *id);
        write_varint(&mut out, *len);
    }
    out
}

/// Total rows a hybrid page expands to.
pub fn expanded_rows(page: &HybridPage) -> Result<u32> {
    let total: u64 = page.runs.iter().map(|(_, len)| *len as u64).sum();
    if total == 0 || total > MAX_STRIPE_ROWS as u64 {
        return Err(Error::LengthMismatch {
            what: "hybrid expanded rows",
        });
    }
    Ok(total as u32)
}

/// Size the stripe buffer for an expansion in the stripe accounting
/// domain.
pub fn plan_stripe(rows: u32) -> usize {
    let rows = rows as u16;
    rows.wrapping_mul(STRIPE_STRIDE) as usize
}

