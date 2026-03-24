//! Dictionary-encoded pages: a word table plus one word id per row.

use crate::bytes::{write_len_slice, write_varint, Reader};
use crate::error::{Error, Result};

#[derive(Debug, Clone)]
pub struct DictPage {
    pub words: Vec<Vec<u8>>,
    pub ids: Vec<u32>,
}

pub fn parse(payload: &[u8]) -> Result<DictPage> {
    let mut r = Reader::new(payload);
    let words = r.varint("dict word count")? as usize;
    if words == 0 || words > 65_536 {
        return Err(Error::LengthMismatch { what: "dict words" });
    }
    let mut word_list = Vec::with_capacity(words);
    for _ in 0..words {
        word_list.push(r.len_slice("dict word")?.to_vec());
    }
    let rows = r.varint("dict row count")? as usize;
    if rows == 0 || rows > 1 << 20 {
        return Err(Error::LengthMismatch { what: "dict rows" });
    }
    let mut ids = Vec::with_capacity(rows);
    for _ in 0..rows {
        let id = r.varint("dict word id")?;
        if id as usize >= word_list.len() {
            return Err(Error::LengthMismatch {
                what: "dict word id",
            });
        }
        ids.push(id);
    }
    Ok(DictPage {
        words: word_list,
        ids,
    })
}

pub fn write(words: &[&[u8]], ids: &[u32]) -> Vec<u8> {
    let mut out = Vec::new();
    write_varint(&mut out, words.len() as u32);
    for w in words {
        write_len_slice(&mut out, w);
    }
    write_varint(&mut out, ids.len() as u32);
    for id in ids {
        write_varint(&mut out, *id);
    }
    out
}

/// Expand to borrowed words, in row order.
pub fn expand(page: &DictPage) -> Vec<&[u8]> {
    page.ids
        .iter()
        .map(|&id| page.words[id as usize].as_slice())
        .collect()
}

