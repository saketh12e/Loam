//! Per-column dictionaries and generation accounting.
//!
//! A dictionary packs a column's distinct values into one contiguous word
//! store; decode cursors borrow that store directly to avoid per-row copies.
//! `refresh` swaps in a new generation and detaches the old store into the
//! retired set, where it is freed once its subscribers drain. The closing
//! session sweep stages near-drained generations and retires them in bulk.

pub mod builder;
pub mod codec;
pub mod frozen;
pub mod global;
pub mod packed;

use std::collections::HashMap;

use crate::error::{Error, Result};

/// Raw parts of a detached word store.
struct RawStore {
    data: *mut u8,
    data_len: usize,
    index: *mut (u32, u32),
    words: usize,
}

impl RawStore {
    unsafe fn dealloc(self) {
        if !self.data.is_null() {
            drop(Box::from_raw(std::ptr::slice_from_raw_parts_mut(
                self.data,
                self.data_len,
            )));
        }
        if !self.index.is_null() {
            drop(Box::from_raw(std::ptr::slice_from_raw_parts_mut(
                self.index, self.words,
            )));
        }
    }

    fn word(&self, i: u32) -> &[u8] {
        let (off, len) = unsafe { *self.index.add(i as usize) };
        unsafe { std::slice::from_raw_parts(self.data.add(off as usize), len as usize) }
    }
}

/// Contiguous storage for a dictionary's words.
pub struct WordStore {
    raw: RawStore,
}

impl WordStore {
    pub fn build(words: &[Vec<u8>]) -> Self {
        let total: usize = words.iter().map(|w| w.len()).sum();
        let mut data = vec![0u8; total.max(1)].into_boxed_slice();
        let data_ptr = data.as_mut_ptr();
        let mut index = vec![(0u32, 0u32); words.len()].into_boxed_slice();
        let index_ptr = index.as_mut_ptr();
        let mut off = 0u32;
        for (i, w) in words.iter().enumerate() {
            unsafe {
                std::ptr::copy_nonoverlapping(w.as_ptr(), data_ptr.add(off as usize), w.len());
                *index_ptr.add(i) = (off, w.len() as u32);
            }
            off += w.len() as u32;
        }
        std::mem::forget(data);
        std::mem::forget(index);
        WordStore {
            raw: RawStore {
                data: data_ptr,
                data_len: total.max(1),
                index: index_ptr,
                words: words.len(),
            },
        }
    }

    pub fn words(&self) -> usize {
        self.raw.words
    }

    pub fn word(&self, i: u32) -> &[u8] {
        self.raw.word(i)
    }

    /// Detach into raw parts; the caller takes over deallocation.
    pub fn into_raw(self) -> RawStoreParts {
        let parts = RawStoreParts {
            data: self.raw.data,
            data_len: self.raw.data_len,
            index: self.raw.index,
            words: self.raw.words,
        };
        std::mem::forget(self);
        parts
    }

    pub fn byte_len(&self) -> usize {
        self.raw.data_len
    }
}

impl Drop for WordStore {
    fn drop(&mut self) {
        unsafe {
            RawStore {
                data: self.raw.data,
                data_len: self.raw.data_len,
                index: self.raw.index,
                words: self.raw.words,
            }
            .dealloc();
        }
        self.raw.data = std::ptr::null_mut();
        self.raw.index = std::ptr::null_mut();
    }
}

/// Public raw handle for a detached store.
#[derive(Clone, Copy)]
pub struct RawStoreParts {
    data: *mut u8,
    data_len: usize,
    index: *mut (u32, u32),
    words: usize,
}

impl RawStoreParts {
    unsafe fn dealloc(self) {
        RawStore {
            data: self.data,
            data_len: self.data_len,
            index: self.index,
            words: self.words,
        }
        .dealloc();
    }
}

/// A decode cursor bound to a word store. Compiled plans hold cursors to
/// avoid re-resolving the dictionary per row.
pub struct DictCursor {
    data: *const u8,
    index: *const (u32, u32),
    words: usize,
}

impl DictCursor {
    /// # Safety
    /// The referenced store must outlive the cursor.
    pub unsafe fn bind(store: &WordStore) -> Self {
        DictCursor {
            data: store.raw.data,
            index: store.raw.index,
            words: store.raw.words,
        }
    }

    pub fn words(&self) -> usize {
        self.words
    }

    /// Read the word for `id`; plans are compiled against stores they trust.
    pub fn word(&self, id: u32) -> Vec<u8> {
        let (off, len) = unsafe { *self.index.add(id as usize) };
        let mut out = vec![0u8; len as usize];
        unsafe {
            std::ptr::copy_nonoverlapping(
                self.data.add(off as usize),
                out.as_mut_ptr(),
                len as usize,
            );
        }
        out
    }
}

/// One dictionary generation for a column.
pub struct Dictionary {
    pub column: u32,
    pub generation: u64,
    store: WordStore,
}

impl Dictionary {
    pub fn new(column: u32, generation: u64, words: Vec<Vec<u8>>) -> Self {
        Dictionary {
            column,
            generation,
            store: WordStore::build(&words),
        }
    }

    pub fn store(&self) -> &WordStore {
        &self.store
    }
}

/// A retired generation awaiting its subscribers.
struct RetiredGen {
    parts: RawStoreParts,
    refs: usize,
}

/// Registry of live dictionaries per column plus generation retirement.
#[derive(Default)]
pub struct DictRegistry {
    current: HashMap<u32, Dictionary>,
    retired: HashMap<u64, RetiredGen>,
}

impl DictRegistry {
    pub fn install(&mut self, dict: Dictionary) {
        self.current.insert(dict.column, dict);
    }

    pub fn current(&self, column: u32) -> Result<&Dictionary> {
        self.current
            .get(&column)
            .ok_or(Error::UnknownColumn { id: column })
    }

    /// Swap in a fresh generation. The outgoing store detaches into the
    /// retired set with the subscriber count handed over by the caller.
    pub fn refresh(&mut self, column: u32, words: Vec<Vec<u8>>, carried_refs: usize) -> u64 {
        let next_gen = self
            .current
            .get(&column)
            .map(|d| d.generation + 1)
            .unwrap_or(1);
        if let Some(old) = self.current.remove(&column) {
            let parts = old.store.into_raw();
            self.retired.insert(
                old.generation,
                RetiredGen {
                    parts,
                    refs: carried_refs,
                },
            );
        }
        self.install(Dictionary::new(column, next_gen, words));
        next_gen
    }

    pub fn retired_refs(&self, generation: u64) -> Option<usize> {
        self.retired.get(&generation).map(|g| g.refs)
    }

    /// Drop one reference to a retired generation; frees its store at zero.
    pub fn release_retired(&mut self, generation: u64) {
        let free = match self.retired.get_mut(&generation) {
            Some(gen) => {
                gen.refs = gen.refs.saturating_sub(1);
                gen.refs == 0
            }
            None => false,
        };
        if free {
            if let Some(gen) = self.retired.remove(&generation) {
                unsafe { gen.parts.dealloc() };
            }
        }
    }

    /// Maintenance pass: free every generation whose refcount has drained.
    pub fn retire_pass(&mut self) {
        let drained: Vec<u64> = self
            .retired
            .iter()
            .filter(|(_, g)| g.refs == 0)
            .map(|(g, _)| *g)
            .collect();
        for generation in drained {
            if let Some(gen) = self.retired.remove(&generation) {
                unsafe { gen.parts.dealloc() };
            }
        }
    }

    /// Stage generations for the closing sweep. Candidates within one
    /// reference of draining are staged with their store parts.
    pub fn sweep_stage(&self) -> Vec<(u64, usize, RawStoreParts)> {
        self.retired
            .iter()
            .filter(|(_, g)| g.refs <= 1)
            .map(|(g, r)| (*g, r.refs, r.parts))
            .collect()
    }

    /// Retire staged generations in bulk.
    pub fn sweep_retire(&mut self, staged: &[(u64, usize, RawStoreParts)]) {
        for (generation, _, parts) in staged {
            unsafe { parts.dealloc() };
            self.retired.remove(generation);
        }
    }

    pub fn live_generations(&self) -> usize {
        self.retired.len()
    }
}

/// Driver for the dictionary-load entry point: parse a dictionary segment
/// (words plus row ids), build the store, and expand the lookups.
pub fn run_dict_driver(raw: &[u8]) -> Result<usize> {
    let mut r = crate::bytes::Reader::new(raw);
    let count = r.varint("dict segment words")? as usize;
    if count == 0 || count > 4096 {
        return Err(Error::LengthMismatch {
            what: "dict segment words",
        });
    }
    let mut words = Vec::with_capacity(count);
    for _ in 0..count {
        words.push(r.len_slice("dict segment word")?.to_vec());
    }
    let rows = r.varint("dict segment rows")? as usize;
    if rows > 1 << 20 {
        return Err(Error::LengthMismatch {
            what: "dict segment rows",
        });
    }
    let store = WordStore::build(&words);
    let mut expanded = 0usize;
    for _ in 0..rows {
        let id = r.varint("dict segment id")?;
        if id as usize >= store.words() {
            return Err(Error::LengthMismatch {
                what: "dict segment id",
            });
        }
        expanded += store.word(id).len();
    }
    Ok(expanded)
}

/// Serialize a dictionary segment for the load entry point.
pub fn write_dict_segment(words: &[&[u8]], ids: &[u32]) -> Vec<u8> {
    let mut out = Vec::new();
    crate::bytes::write_varint(&mut out, words.len() as u32);
    for w in words {
        crate::bytes::write_len_slice(&mut out, w);
    }
    crate::bytes::write_varint(&mut out, ids.len() as u32);
    for id in ids {
        crate::bytes::write_varint(&mut out, *id);
    }
    out
}

