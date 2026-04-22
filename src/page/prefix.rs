//! Prefix-compressed string pages: dictionary-light storage for UTF8
//! columns whose values share long prefixes (paths, keys, URLs).
//!
//! Groups carry a shared prefix plus per-row suffixes. A group may declare
//! an *empty* prefix to inherit the previous group's prefix — a compaction
//! for runs of near-identical paths. Word assembly happens in a fixed stack
//! scratch sized for the format's word cap.

use crate::bytes::{write_len_slice, write_varint, Reader};
use crate::error::{Error, Result};

/// Maximum bytes of one assembled word.
const WORD_SCRATCH: usize = 128;

/// Format tag for prefix pages inside `PG` containers.
pub const ENC_PREFIX: u32 = 8;

/// Encode values as (prefix-run) groups: a shared prefix per group plus
/// per-row suffixes. An empty `group_prefix` inherits the previous group's.
pub fn encode_groups(groups: &[(Vec<u8>, Vec<Vec<u8>>)]) -> Result<Vec<u8>> {
    if groups.is_empty() {
        return Err(Error::LengthMismatch {
            what: "prefix page",
        });
    }
    let mut out = Vec::new();
    write_varint(&mut out, groups.len() as u32);
    for (prefix, words) in groups {
        if prefix.len() > WORD_SCRATCH {
            return Err(Error::LengthMismatch {
                what: "prefix length",
            });
        }
        write_len_slice(&mut out, prefix);
        write_varint(&mut out, words.len() as u32);
        for w in words {
            if w.len() > WORD_SCRATCH {
                return Err(Error::LengthMismatch {
                    what: "prefix word",
                });
            }
            write_len_slice(&mut out, w);
        }
    }
    Ok(out)
}

/// Convenience: encode one explicit group.
pub fn encode(values: &[Vec<u8>], group: usize) -> Result<Vec<u8>> {
    if values.is_empty() || group == 0 {
        return Err(Error::LengthMismatch {
            what: "prefix page",
        });
    }
    let mut groups = Vec::new();
    let mut i = 0usize;
    while i < values.len() {
        let end = (i + group).min(values.len());
        let prefix = common_prefix(&values[i..end]);
        let words: Vec<Vec<u8>> = values[i..end]
            .iter()
            .map(|v| v[prefix.len()..].to_vec())
            .collect();
        groups.push((prefix, words));
        i = end;
    }
    encode_groups(&groups)
}

fn common_prefix(values: &[Vec<u8>]) -> Vec<u8> {
    if values.is_empty() {
        return Vec::new();
    }
    let mut prefix = values[0].clone();
    for v in &values[1..] {
        let shared = prefix
            .iter()
            .zip(v.iter())
            .take_while(|(a, b)| a == b)
            .count();
        prefix.truncate(shared);
    }
    prefix
}

/// Decode a prefix page into `rows` values. Word assembly uses a fixed
/// stack scratch sized for the format's word cap.
pub fn decode(raw: &[u8], rows: u32) -> Result<Vec<Vec<u8>>> {
    let mut r = Reader::new(raw);
    let groups = r.varint("prefix groups")? as usize;
    if groups == 0 || groups > 1024 {
        return Err(Error::LengthMismatch {
            what: "prefix groups",
        });
    }
    let mut out: Vec<Vec<u8>> = Vec::with_capacity(rows as usize);
    let mut scratch = [0u8; WORD_SCRATCH];
    let mut current_prefix = 0usize;
    for _ in 0..groups {
        let prefix = r.len_slice("prefix bytes")?;
        if prefix.is_empty() {
            // Inherit the previous group's prefix.
        } else {
            if prefix.len() > WORD_SCRATCH {
                return Err(Error::LengthMismatch {
                    what: "prefix length",
                });
            }
            scratch[..prefix.len()].copy_from_slice(prefix);
            current_prefix = prefix.len();
        }
        let words = r.varint("prefix words")? as usize;
        if words == 0 || words > 4096 {
            return Err(Error::LengthMismatch {
                what: "prefix words",
            });
        }
        for _ in 0..words {
            let suffix = r.len_slice("prefix suffix")?;
            if suffix.len() > WORD_SCRATCH {
                return Err(Error::LengthMismatch {
                    what: "prefix suffix",
                });
            }
            unsafe {
                std::ptr::copy_nonoverlapping(
                    suffix.as_ptr(),
                    scratch.as_mut_ptr().add(current_prefix),
                    suffix.len(),
                );
            }
            out.push(scratch[..current_prefix + suffix.len()].to_vec());
            if out.len() == rows as usize {
                return Ok(out);
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let values: Vec<Vec<u8>> = vec![
            b"data/2026/01/a".to_vec(),
            b"data/2026/01/b".to_vec(),
            b"logs/x".to_vec(),
        ];
        let raw = encode(&values, 2).unwrap();
        assert_eq!(decode(&raw, 3).unwrap(), values);
    }

    #[test]
    fn explicit_groups_round_trip() {
        let raw = encode_groups(&[
            (b"data/".to_vec(), vec![b"a".to_vec(), b"b".to_vec()]),
            (b"log/".to_vec(), vec![b"x".to_vec()]),
        ])
        .unwrap();
        assert_eq!(
            decode(&raw, 3).unwrap(),
            vec![b"data/a".to_vec(), b"data/b".to_vec(), b"log/x".to_vec()]
        );
    }

    #[test]
    fn inherit_group_uses_previous_prefix() {
        let raw = encode_groups(&[
            (b"data/".to_vec(), vec![b"a".to_vec()]),
            (Vec::new(), vec![b"b".to_vec()]),
        ])
        .unwrap();
        assert_eq!(
            decode(&raw, 2).unwrap(),
            vec![b"data/a".to_vec(), b"data/b".to_vec()]
        );
    }

    #[test]
    fn rejects_empty() {
        assert!(encode(&[], 2).is_err());
    }
}
