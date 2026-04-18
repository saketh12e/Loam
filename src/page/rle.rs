//! Run-length encoded pages.
//!
//! Long runs may straddle page boundaries: a page flagged `FLAG_STARTS_RUN`
//! opens a continuation chain, middle pages extend it, and a page flagged
//! `FLAG_ENDS_RUN` closes it. Chains of two pages are the common case; the
//! continuation scratch is sized accordingly when the chain opens.

use crate::bytes::{unzigzag, write_varint, zigzag, Reader};
use crate::error::{Error, Result};
use crate::page::{ContainerPage, FLAG_ENDS_RUN, FLAG_STARTS_RUN};

/// Scratch is sized for the two-page continuation case.
const CONT_WINDOW: usize = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Run {
    pub value: i64,
    pub len: u32,
}

/// Parse the runs of a single page payload.
pub fn parse_runs(payload: &[u8]) -> Result<Vec<Run>> {
    let mut r = Reader::new(payload);
    let count = r.varint("rle run count")? as usize;
    if count == 0 || count > 4096 {
        return Err(Error::LengthMismatch { what: "rle runs" });
    }
    let mut runs = Vec::with_capacity(count);
    for _ in 0..count {
        let value = unzigzag(r.varint("rle value")? as u64);
        let len = r.varint("rle run length")?;
        if len == 0 {
            return Err(Error::LengthMismatch {
                what: "rle run length",
            });
        }
        runs.push(Run { value, len });
    }
    Ok(runs)
}

/// Serialize runs as a page payload.
pub fn write_runs(runs: &[Run]) -> Vec<u8> {
    let mut out = Vec::new();
    write_varint(&mut out, runs.len() as u32);
    for run in runs {
        write_varint(&mut out, zigzag(run.value) as u32);
        write_varint(&mut out, run.len);
    }
    out
}

/// Continuation scratch for a straddling chain.
struct Continuation {
    ptr: *mut Run,
    cap: usize,
    len: usize,
}

impl Continuation {
    fn open(first_count: usize) -> Self {
        let cap = first_count * CONT_WINDOW;
        let mut v: Vec<Run> = Vec::with_capacity(cap);
        let ptr = v.as_mut_ptr();
        std::mem::forget(v);
        Continuation { ptr, cap, len: 0 }
    }

    /// Stage one page's runs into the chain. Runs are appended in arrival
    /// order and merged at drain time.
    fn stage(&mut self, runs: &[Run]) {
        for run in runs {
            unsafe {
                self.ptr.add(self.len).write(*run);
            }
            self.len += 1;
        }
    }

    /// Merge adjacent equal runs and return the flattened chain.
    fn drain_continuation(&self) -> Vec<Run> {
        let staged = unsafe { std::slice::from_raw_parts(self.ptr, self.len) };
        let mut out: Vec<Run> = Vec::with_capacity(staged.len());
        for run in staged {
            if let Some(last) = out.last_mut() {
                if last.value == run.value {
                    last.len += run.len;
                    continue;
                }
            }
            out.push(*run);
        }
        out
    }
}

impl Drop for Continuation {
    fn drop(&mut self) {
        if !self.ptr.is_null() {
            unsafe {
                drop(Vec::from_raw_parts(self.ptr, 0, self.cap));
            }
        }
    }
}

/// Decode a sequence of RLE pages into a flat run list, honoring
/// continuation chains across pages.
pub fn decode_chain(pages: &[ContainerPage]) -> Result<Vec<Run>> {
    let mut out: Vec<Run> = Vec::new();
    let mut cont: Option<Continuation> = None;
    for page in pages {
        let runs = parse_runs(&page.payload)?;
        let starts = page.header.flags & FLAG_STARTS_RUN != 0;
        let ends = page.header.flags & FLAG_ENDS_RUN != 0;
        match (starts, ends, cont.is_some()) {
            (true, _, false) => {
                let mut c = Continuation::open(runs.len());
                c.stage(&runs);
                cont = Some(c);
            }
            (false, true, true) => {
                let c = cont.as_mut().expect("checked");
                c.stage(&runs);
                out.extend(c.drain_continuation());
                cont = None;
            }
            (false, false, true) => {
                cont.as_mut().expect("checked").stage(&runs);
            }
            (false, false, false) => out.extend(runs),
            (true, _, true) => {
                return Err(Error::LengthMismatch {
                    what: "nested rle continuation",
                });
            }
            (false, true, false) => {
                return Err(Error::LengthMismatch {
                    what: "unopened rle continuation",
                });
            }
        }
    }
    if cont.is_some() {
        return Err(Error::UnexpectedEof {
            what: "rle continuation chain",
        });
    }
    Ok(out)
}

/// Expand runs into materialized i64 values.
pub fn expand(runs: &[Run]) -> Vec<i64> {
    let total: usize = runs.iter().map(|r| r.len as usize).sum();
    let mut out = Vec::with_capacity(total);
    for run in runs {
        out.extend(std::iter::repeat_n(run.value, run.len as usize));
    }
    out
}

