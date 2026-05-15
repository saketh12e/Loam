//! Frozen dictionaries: immutable, lookup-optimized word tables built once
//! from a sorted word list, for columns whose domain never changes.

use crate::error::{Error, Result};

/// A frozen dictionary: sorted words plus binary-search lookup.
#[derive(Debug, Clone)]
pub struct FrozenDict {
    words: Vec<Vec<u8>>,
}

impl FrozenDict {
    /// Build from an unsorted word list (sorted + deduped internally).
    pub fn build(mut words: Vec<Vec<u8>>) -> Result<Self> {
        if words.is_empty() {
            return Err(Error::LengthMismatch {
                what: "frozen dict",
            });
        }
        words.sort();
        words.dedup();
        Ok(FrozenDict { words })
    }

    /// Binary-search lookup; returns the word id.
    pub fn lookup(&self, value: &[u8]) -> Option<u32> {
        self.words
            .binary_search_by(|w| w.as_slice().cmp(value))
            .ok()
            .map(|i| i as u32)
    }

    pub fn word(&self, id: u32) -> Option<&[u8]> {
        self.words.get(id as usize).map(|w| w.as_slice())
    }

    pub fn words(&self) -> usize {
        self.words.len()
    }

    /// Range of ids whose words start with `prefix`.
    pub fn prefix_range(&self, prefix: &[u8]) -> (u32, u32) {
        let start = self.words.partition_point(|w| w.as_slice() < prefix) as u32;
        let mut end = start;
        while (end as usize) < self.words.len() && self.words[end as usize].starts_with(prefix) {
            end += 1;
        }
        (start, end)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_sorts_and_dedupes() {
        let d = FrozenDict::build(vec![b"b".to_vec(), b"a".to_vec(), b"b".to_vec()]).unwrap();
        assert_eq!(d.words(), 2);
        assert_eq!(d.lookup(b"a"), Some(0));
        assert_eq!(d.lookup(b"b"), Some(1));
        assert_eq!(d.word(1), Some(b"b".as_slice()));
    }

    #[test]
    fn prefix_ranges() {
        let d = FrozenDict::build(vec![b"ab".to_vec(), b"abc".to_vec(), b"b".to_vec()]).unwrap();
        assert_eq!(d.prefix_range(b"ab"), (0, 2));
        assert_eq!(d.prefix_range(b"z"), (3, 3));
    }

    #[test]
    fn rejects_empty() {
        assert!(FrozenDict::build(vec![]).is_err());
    }
}
