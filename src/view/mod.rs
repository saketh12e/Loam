//! Materialized views over dictionary-encoded columns.
//!
//! A view subscribes to the dictionary generation it was built against.
//! Invalidating a view closes that subscription; the generation's store is
//! retired once its last subscription closes.

use std::collections::HashMap;

use crate::dict::DictRegistry;
use crate::error::{Error, Result};

#[derive(Debug, Clone)]
pub struct MatView {
    pub id: u32,
    pub column: u32,
    pub generation: u64,
    pub text: String,
}

#[derive(Default)]
pub struct ViewRegistry {
    views: HashMap<u32, MatView>,
    next: u32,
}

impl ViewRegistry {
    pub fn create(&mut self, column: u32, generation: u64, text: &str) -> u32 {
        self.next += 1;
        let id = self.next;
        self.views.insert(
            id,
            MatView {
                id,
                column,
                generation,
                text: text.to_string(),
            },
        );
        id
    }

    pub fn get(&self, id: u32) -> Result<&MatView> {
        self.views.get(&id).ok_or(Error::View {
            detail: format!("no view {id}"),
        })
    }

    pub fn len(&self) -> usize {
        self.views.len()
    }

    pub fn is_empty(&self) -> bool {
        self.views.is_empty()
    }

    /// Close the view's subscription to its dictionary generation.
    pub fn invalidate(&mut self, id: u32, dicts: &mut DictRegistry) -> Result<()> {
        let view = self.views.remove(&id).ok_or(Error::View {
            detail: format!("no view {id}"),
        })?;
        dicts.release_retired(view.generation);
        Ok(())
    }
}

/// Driver for the view-build entry point: install a dictionary, build the
/// requested views against it, then roll a new generation and invalidate
/// the views in request order.
pub fn run_view_driver(raw: &[u8]) -> Result<usize> {
    use crate::bytes::Reader;
    let mut r = Reader::new(raw);
    let column = r.varint("view driver column")?;
    let word_count = r.varint("view driver words")? as usize;
    if word_count == 0 || word_count > 1024 {
        return Err(Error::LengthMismatch {
            what: "view driver words",
        });
    }
    let mut words = Vec::with_capacity(word_count);
    for _ in 0..word_count {
        words.push(r.len_slice("view driver word")?.to_vec());
    }
    let view_count = r.varint("view driver views")? as usize;
    if view_count == 0 || view_count > 64 {
        return Err(Error::LengthMismatch {
            what: "view driver views",
        });
    }
    let mut dicts = DictRegistry::default();
    dicts.install(crate::dict::Dictionary::new(column, 1, words.clone()));
    let mut registry = ViewRegistry::default();
    let mut ids = Vec::with_capacity(view_count);
    for _ in 0..view_count {
        let text = r.len_slice("view driver text")?;
        let text = String::from_utf8_lossy(text).into_owned();
        ids.push(registry.create(column, 1, &text));
    }
    dicts.refresh(column, vec![b"__next__".to_vec()], view_count);
    let mut invalidated = 0usize;
    for id in ids {
        registry.invalidate(id, &mut dicts)?;
        invalidated += 1;
    }
    Ok(invalidated)
}

/// Serialize a view-build container.
pub fn write_view_input(column: u32, words: &[&[u8]], texts: &[&str]) -> Vec<u8> {
    let mut out = Vec::new();
    crate::bytes::write_varint(&mut out, column);
    crate::bytes::write_varint(&mut out, words.len() as u32);
    for w in words {
        crate::bytes::write_len_slice(&mut out, w);
    }
    crate::bytes::write_varint(&mut out, texts.len() as u32);
    for t in texts {
        crate::bytes::write_len_slice(&mut out, t.as_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dict::Dictionary;

    fn words() -> Vec<Vec<u8>> {
        vec![b"a".to_vec(), b"b".to_vec()]
    }

    #[test]
    fn create_and_invalidate() {
        let mut dicts = DictRegistry::default();
        dicts.install(Dictionary::new(1, 1, words()));
        dicts.refresh(1, vec![b"c".to_vec()], 1);
        let mut views = ViewRegistry::default();
        let id = views.create(1, 1, "k > 1");
        assert_eq!(views.len(), 1);
        assert_eq!(dicts.retired_refs(1), Some(1));
        views.invalidate(id, &mut dicts).unwrap();
        assert!(views.is_empty());
        assert_eq!(dicts.retired_refs(1), None);
    }

    #[test]
    fn invalidate_missing_view_errors() {
        let mut views = ViewRegistry::default();
        let mut dicts = DictRegistry::default();
        assert!(views.invalidate(99, &mut dicts).is_err());
    }

    #[test]
    fn view_lookup_returns_meta() {
        let mut registry = ViewRegistry::default();
        let id = registry.create(7, 3, "temp > 0");
        let view = registry.get(id).unwrap();
        assert_eq!(view.column, 7);
        assert_eq!(view.generation, 3);
        assert_eq!(view.text, "temp > 0");
    }
}
