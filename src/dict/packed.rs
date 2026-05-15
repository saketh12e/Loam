//! Packed dictionary storage with front-coding for sorted word tables.
//!
//! Sorted words share prefixes; the packed form stores each word as a shared
//! prefix length plus the suffix bytes. Decode walks the table once and
//! rebuilds words into the contiguous store layout.

use crate::bytes::{write_len_slice, write_varint, Reader};
use crate::error::{Error, Result};

/// Encode sorted words with front-coding.
pub fn encode(sorted_words: &[Vec<u8>]) -> Result<Vec<u8>> {
    if sorted_words.is_empty() {
        return Err(Error::LengthMismatch {
            what: "packed dict",
        });
    }
    let mut out = Vec::new();
    write_varint(&mut out, sorted_words.len() as u32);
    let mut prev: &[u8] = &[];
    for w in sorted_words {
        let shared = prev
            .iter()
            .zip(w.iter())
            .take_while(|(a, b)| a == b)
            .count();
        write_varint(&mut out, shared as u32);
        write_len_slice(&mut out, &w[shared..]);
        prev = w;
    }
    Ok(out)
}

/// Decode a front-coded table back into words.
pub fn decode(raw: &[u8]) -> Result<Vec<Vec<u8>>> {
    let mut r = Reader::new(raw);
    let count = r.varint("packed dict words")? as usize;
    if count == 0 || count > 1 << 16 {
        return Err(Error::LengthMismatch {
            what: "packed dict words",
        });
    }
    let mut words = Vec::with_capacity(count);
    let mut prev: Vec<u8> = Vec::new();
    for _ in 0..count {
        let shared = r.varint("packed shared prefix")? as usize;
        if shared > prev.len() {
            return Err(Error::LengthMismatch {
                what: "packed shared prefix",
            });
        }
        let suffix = r.len_slice("packed suffix")?;
        let mut word = prev[..shared].to_vec();
        word.extend_from_slice(suffix);
        words.push(word.clone());
        prev = word;
    }
    Ok(words)
}

/// Estimated saving of front-coding versus plain storage, in bytes.
pub fn estimated_saving(sorted_words: &[Vec<u8>]) -> usize {
    let mut saving = 0usize;
    let mut prev: &[u8] = &[];
    for w in sorted_words {
        let shared = prev
            .iter()
            .zip(w.iter())
            .take_while(|(a, b)| a == b)
            .count();
        saving += shared.saturating_sub(2);
        prev = w;
    }
    saving
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words() -> Vec<Vec<u8>> {
        vec![
            b"region:ap".to_vec(),
            b"region:eu".to_vec(),
            b"region:us".to_vec(),
            b"region:us:east".to_vec(),
        ]
    }

    #[test]
    fn round_trip() {
        let raw = encode(&words()).unwrap();
        assert_eq!(decode(&raw).unwrap(), words());
    }

    #[test]
    fn saves_on_shared_prefixes() {
        assert!(estimated_saving(&words()) > 8);
    }

    #[test]
    fn rejects_overshoot_prefix() {
        let mut raw = Vec::new();
        write_varint(&mut raw, 1);
        write_varint(&mut raw, 9);
        write_len_slice(&mut raw, b"x");
        assert!(decode(&raw).is_err());
    }
}
