//! Dictionary construction from raw value streams.
//!
//! The builder collects distinct values in first-seen order, emits the word
//! table plus per-row ids, and reports the compression ratio so the writer
//! can decide whether dictionary encoding pays off for a column.

use std::collections::HashMap;

use crate::error::{Error, Result};

#[derive(Debug, Default)]
pub struct DictBuilder {
    words: Vec<Vec<u8>>,
    ids: Vec<u32>,
    seen: HashMap<Vec<u8>, u32>,
}

impl DictBuilder {
    pub fn new() -> Self {
        DictBuilder::default()
    }

    /// Add one value; returns its word id.
    pub fn push(&mut self, value: &[u8]) -> u32 {
        if let Some(&id) = self.seen.get(value) {
            self.ids.push(id);
            return id;
        }
        let id = self.words.len() as u32;
        self.words.push(value.to_vec());
        self.seen.insert(value.to_vec(), id);
        self.ids.push(id);
        id
    }

    pub fn push_all(&mut self, values: &[&[u8]]) {
        for v in values {
            self.push(v);
        }
    }

    pub fn words(&self) -> &[Vec<u8>] {
        &self.words
    }

    pub fn ids(&self) -> &[u32] {
        &self.ids
    }

    /// Ratio of distinct words to rows; the writer dictionaries a column
    /// when this is below its threshold.
    pub fn density(&self) -> f64 {
        if self.ids.is_empty() {
            return 1.0;
        }
        self.words.len() as f64 / self.ids.len() as f64
    }

    /// Encode into the page payload form (`page::dict`).
    pub fn into_payload(self) -> Result<Vec<u8>> {
        if self.words.is_empty() {
            return Err(Error::LengthMismatch {
                what: "dictionary build",
            });
        }
        let words: Vec<&[u8]> = self.words.iter().map(|w| w.as_slice()).collect();
        Ok(crate::page::dict::write(&words, &self.ids))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dedupes_in_first_seen_order() {
        let mut b = DictBuilder::new();
        assert_eq!(b.push(b"red"), 0);
        assert_eq!(b.push(b"blue"), 1);
        assert_eq!(b.push(b"red"), 0);
        assert_eq!(b.ids(), &[0, 1, 0]);
        assert_eq!(b.words().len(), 2);
    }

    #[test]
    fn density_tracks_repetition() {
        let mut b = DictBuilder::new();
        b.push_all(&[b"x", b"x", b"x", b"y"]);
        assert!((b.density() - 0.5).abs() < 1e-9);
    }

    #[test]
    fn payload_decodes_back() {
        let mut b = DictBuilder::new();
        b.push_all(&[b"a", b"b", b"a"]);
        let payload = b.into_payload().unwrap();
        let page = crate::page::dict::parse(&payload).unwrap();
        assert_eq!(page.ids, vec![0, 1, 0]);
    }

    #[test]
    fn empty_builder_rejected() {
        assert!(DictBuilder::new().into_payload().is_err());
    }
}
