//! Dictionary segment codec: the on-disk form dictionaries travel in when
//! exported between files (distinct from page payloads).

use crate::bytes::{write_varint, Reader};
use crate::error::{Error, Result};

pub const SEG_MAGIC: &[u8; 2] = b"DC";

/// Encode words as an export segment.
pub fn encode(words: &[Vec<u8>]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(SEG_MAGIC);
    write_varint(&mut out, words.len() as u32);
    for w in words {
        crate::bytes::write_len_slice(&mut out, w);
    }
    out
}

/// Decode an export segment back into words.
pub fn decode(raw: &[u8]) -> Result<Vec<Vec<u8>>> {
    let mut r = Reader::new(raw);
    if r.take(2, "dict segment magic")? != SEG_MAGIC {
        return Err(Error::BadMagic {
            what: "dict segment",
        });
    }
    let count = r.varint("dict segment words")? as usize;
    if count == 0 || count > 1 << 16 {
        return Err(Error::LengthMismatch {
            what: "dict segment words",
        });
    }
    let mut words = Vec::with_capacity(count);
    for _ in 0..count {
        words.push(r.len_slice("dict segment word")?.to_vec());
    }
    Ok(words)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let words = vec![b"alpha".to_vec(), b"beta".to_vec()];
        assert_eq!(decode(&encode(&words)).unwrap(), words);
    }

    #[test]
    fn rejects_bad_magic() {
        assert!(decode(b"XX\x01a").is_err());
    }
}
