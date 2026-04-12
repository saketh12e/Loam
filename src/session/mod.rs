//! Session state and the script operation language.
//!
//! Embedded callers drive multi-operation workflows as scripts: ingest
//! chunks, hold and fork page views, install and refresh dictionaries, run
//! dictionary queries, manage materialized views, and close with a sweep.
//! Scripts are length-prefixed operation records; this module both parses
//! and serializes them.

pub mod checkpoint;
pub mod meter;
pub mod replay;
pub mod transactions;
pub mod wal;

use crate::batch::View;
use crate::bytes::{write_len_slice, write_varint, Reader};
use crate::cache::PageCache;
use crate::dict::{DictRegistry, Dictionary, RawStoreParts};
use crate::error::{Error, Result};
use crate::exec::decode_dict_column;
use crate::plan::{PlanCache, PlanNode};
use crate::view::ViewRegistry;

/// Operation codes.
pub mod ops {
    pub const INGEST: u8 = 0x01;
    pub const HOLD: u8 = 0x02;
    pub const FORK: u8 = 0x03;
    pub const READ: u8 = 0x04;
    pub const INSTALL_DICT: u8 = 0x05;
    pub const REFRESH_DICT: u8 = 0x06;
    pub const QUERY_DICT: u8 = 0x07;
    pub const CREATE_VIEW: u8 = 0x08;
    pub const INVALIDATE_VIEW: u8 = 0x09;
    pub const BEGIN_SWEEP: u8 = 0x0a;
    pub const CLOSE: u8 = 0x0b;
}

/// Session state shared across a script.
pub struct Session {
    pub cache: PageCache,
    pub dicts: DictRegistry,
    pub views: ViewRegistry,
    pub plans: PlanCache,
    held: Vec<View>,
    sweep: Option<Vec<(u64, usize, RawStoreParts)>>,
    /// Rolling checksum of bytes read through views, for result accounting.
    pub read_checksum: u64,
}

impl Session {
    pub fn new(cache_budget: usize) -> Self {
        Session {
            cache: PageCache::new(cache_budget),
            dicts: DictRegistry::default(),
            views: ViewRegistry::default(),
            plans: PlanCache::default(),
            held: Vec::new(),
            sweep: None,
            read_checksum: 0,
        }
    }

    fn ingest(&mut self, page: u32, bytes: &[u8]) -> Result<()> {
        self.cache.advance_epoch();
        self.cache.insert(page, bytes)?;
        Ok(())
    }

    fn hold(&mut self, page: u32) -> Result<()> {
        let view = self.cache.hold(page)?;
        self.held.push(view);
        Ok(())
    }

    fn fork(&mut self, index: u32) -> Result<()> {
        let view = self
            .held
            .get(index as usize)
            .ok_or(Error::Session {
                detail: format!("no held view {index}"),
            })?
            .fork();
        self.held.push(view);
        Ok(())
    }

    fn read(&mut self, index: u32) -> Result<()> {
        let view = self.held.get(index as usize).ok_or(Error::Session {
            detail: format!("no held view {index}"),
        })?;
        let bytes = view.read();
        for b in bytes {
            self.read_checksum = self.read_checksum.wrapping_mul(31).wrapping_add(b as u64);
        }
        Ok(())
    }

    fn install_dict(&mut self, column: u32, words: Vec<Vec<u8>>) {
        self.dicts.install(Dictionary::new(column, 1, words));
    }

    fn refresh_dict(&mut self, column: u32, words: Vec<Vec<u8>>, carried_refs: usize) {
        self.dicts.refresh(column, words, carried_refs);
        // Maintenance: retire drained generations eagerly.
        self.dicts.retire_pass();
    }

    fn query_dict(&mut self, column: u32, encoding: u32, ids: &[u32]) -> Result<usize> {
        let nodes = vec![
            PlanNode::Scan { column },
            PlanNode::Filter {
                text: "k > 0".to_string(),
            },
        ];
        let compiled = self
            .plans
            .get_or_compile(column, encoding, &nodes, &self.dicts)?;
        let mut decoded = 0usize;
        for (_, cursor) in &compiled.cursors {
            decoded += decode_dict_column(cursor, ids).len();
        }
        Ok(decoded)
    }

    fn begin_sweep(&mut self) {
        self.sweep = Some(self.dicts.sweep_stage());
    }

    fn close(&mut self) {
        self.held.clear();
        if let Some(staged) = self.sweep.take() {
            self.dicts.sweep_retire(&staged);
        }
        self.dicts.retire_pass();
    }

    /// Execute a script of operations.
    pub fn run_script(&mut self, raw: &[u8]) -> Result<()> {
        let mut r = Reader::new(raw);
        while !r.is_empty() {
            let op = r.u8("script op")?;
            match op {
                ops::INGEST => {
                    let page = r.varint("ingest page")?;
                    let bytes = r.len_slice("ingest bytes")?;
                    self.ingest(page, bytes)?;
                }
                ops::HOLD => {
                    let page = r.varint("hold page")?;
                    self.hold(page)?;
                }
                ops::FORK => {
                    let index = r.varint("fork index")?;
                    self.fork(index)?;
                }
                ops::READ => {
                    let index = r.varint("read index")?;
                    self.read(index)?;
                }
                ops::INSTALL_DICT => {
                    let column = r.varint("dict column")?;
                    let words = read_words(&mut r)?;
                    self.install_dict(column, words);
                }
                ops::REFRESH_DICT => {
                    let column = r.varint("refresh column")?;
                    let carried = r.varint("refresh refs")? as usize;
                    let words = read_words(&mut r)?;
                    self.refresh_dict(column, words, carried);
                }
                ops::QUERY_DICT => {
                    let column = r.varint("query column")?;
                    let encoding = r.varint("query encoding")?;
                    let count = r.varint("query ids")? as usize;
                    if count > 1 << 16 {
                        return Err(Error::LengthMismatch { what: "query ids" });
                    }
                    let mut ids = Vec::with_capacity(count);
                    for _ in 0..count {
                        ids.push(r.varint("query id")?);
                    }
                    self.query_dict(column, encoding, &ids)?;
                }
                ops::CREATE_VIEW => {
                    let column = r.varint("view column")?;
                    let generation = r.varint("view generation")? as u64;
                    let text = r.len_slice("view text")?;
                    let text = String::from_utf8_lossy(text).into_owned();
                    self.views.create(column, generation, &text);
                }
                ops::INVALIDATE_VIEW => {
                    let id = r.varint("invalidate id")?;
                    self.views.invalidate(id, &mut self.dicts)?;
                }
                ops::BEGIN_SWEEP => self.begin_sweep(),
                ops::CLOSE => {
                    self.close();
                    return Ok(());
                }
                other => {
                    return Err(Error::Session {
                        detail: format!("unknown op {other:#x}"),
                    })
                }
            }
        }
        Ok(())
    }
}

fn read_words(r: &mut Reader<'_>) -> Result<Vec<Vec<u8>>> {
    let count = r.varint("word count")? as usize;
    if count == 0 || count > 4096 {
        return Err(Error::LengthMismatch { what: "dict words" });
    }
    let mut words = Vec::with_capacity(count);
    for _ in 0..count {
        let w = r.len_slice("dict word")?;
        if w.len() > 1 << 12 {
            return Err(Error::LengthMismatch { what: "dict word" });
        }
        words.push(w.to_vec());
    }
    Ok(words)
}

/// Script builder used by tooling, tests, and corpus seeds.
#[derive(Default)]
pub struct ScriptWriter {
    out: Vec<u8>,
}

impl ScriptWriter {
    pub fn new() -> Self {
        ScriptWriter::default()
    }

    pub fn ingest(mut self, page: u32, bytes: &[u8]) -> Self {
        self.out.push(ops::INGEST);
        write_varint(&mut self.out, page);
        write_len_slice(&mut self.out, bytes);
        self
    }

    pub fn hold(mut self, page: u32) -> Self {
        self.out.push(ops::HOLD);
        write_varint(&mut self.out, page);
        self
    }

    pub fn fork(mut self, index: u32) -> Self {
        self.out.push(ops::FORK);
        write_varint(&mut self.out, index);
        self
    }

    pub fn read(mut self, index: u32) -> Self {
        self.out.push(ops::READ);
        write_varint(&mut self.out, index);
        self
    }

    pub fn install_dict(mut self, column: u32, words: &[&[u8]]) -> Self {
        self.out.push(ops::INSTALL_DICT);
        write_varint(&mut self.out, column);
        write_words(&mut self.out, words);
        self
    }

    pub fn refresh_dict(mut self, column: u32, carried_refs: u32, words: &[&[u8]]) -> Self {
        self.out.push(ops::REFRESH_DICT);
        write_varint(&mut self.out, column);
        write_varint(&mut self.out, carried_refs);
        write_words(&mut self.out, words);
        self
    }

    pub fn query_dict(mut self, column: u32, encoding: u32, ids: &[u32]) -> Self {
        self.out.push(ops::QUERY_DICT);
        write_varint(&mut self.out, column);
        write_varint(&mut self.out, encoding);
        write_varint(&mut self.out, ids.len() as u32);
        for id in ids {
            write_varint(&mut self.out, *id);
        }
        self
    }

    pub fn create_view(mut self, column: u32, generation: u32, text: &str) -> Self {
        self.out.push(ops::CREATE_VIEW);
        write_varint(&mut self.out, column);
        write_varint(&mut self.out, generation);
        write_len_slice(&mut self.out, text.as_bytes());
        self
    }

    pub fn invalidate_view(mut self, id: u32) -> Self {
        self.out.push(ops::INVALIDATE_VIEW);
        write_varint(&mut self.out, id);
        self
    }

    pub fn begin_sweep(mut self) -> Self {
        self.out.push(ops::BEGIN_SWEEP);
        self
    }

    pub fn close(mut self) -> Self {
        self.out.push(ops::CLOSE);
        self
    }

    pub fn finish(self) -> Vec<u8> {
        self.out
    }
}

fn write_words(out: &mut Vec<u8>, words: &[&[u8]]) {
    write_varint(out, words.len() as u32);
    for w in words {
        write_len_slice(out, w);
    }
}

/// Driver for the session entry point.
pub fn run_session_driver(raw: &[u8]) -> Result<u64> {
    let mut session = Session::new(1 << 12);
    session.run_script(raw)?;
    Ok(session.read_checksum)
}

