//! Two-tier cache front: a small hot tier in front of the main page cache,
//! with promotion on second touch and batch warming driven by prefetch.
//!
//! The hot tier exists because scans hit the same few pages repeatedly in
//! short windows; keeping them out of the clock sweep's way avoids needless
//! decode churn. Batch warming plans evictions in bulk.

use std::collections::HashMap;

use crate::bytes::{write_len_slice, write_varint, Reader};
use crate::error::{Error, Result};

/// Zero-copy borrow of a hot page.
pub struct WarmView {
    ptr: *const u8,
    len: usize,
}

impl WarmView {
    /// Copy the viewed bytes out.
    pub fn read(&self) -> Vec<u8> {
        let mut out = vec![0u8; self.len];
        unsafe {
            std::ptr::copy_nonoverlapping(self.ptr, out.as_mut_ptr(), self.len);
        }
        out
    }
}

struct HotEntry {
    ptr: *mut u8,
    bytes: usize,
    touches: u32,
}

pub struct TieredCache {
    hot: HashMap<u32, HotEntry>,
    hot_budget: usize,
    hot_used: usize,
    /// Touch snapshot for the open batch: page -> touches at batch start.
    batch_plan: Option<Vec<(u32, u32)>>,
}

impl TieredCache {
    pub fn new(hot_budget: usize) -> Self {
        TieredCache {
            hot: HashMap::new(),
            hot_budget: hot_budget.max(1 << 10),
            hot_used: 0,
            batch_plan: None,
        }
    }

    /// Fetch a page, promoting it within the tier.
    pub fn fetch(&mut self, id: u32) -> Option<WarmView> {
        let entry = self.hot.get_mut(&id)?;
        entry.touches += 1;
        Some(WarmView {
            ptr: entry.ptr,
            len: entry.bytes,
        })
    }

    /// Warm a single page outside a batch.
    pub fn warm(&mut self, id: u32, bytes: &[u8]) -> Result<()> {
        if bytes.is_empty() || bytes.len() > 1 << 20 {
            return Err(Error::BudgetExceeded {
                what: "tiered page",
            });
        }
        if self.hot.contains_key(&id) {
            return Ok(());
        }
        self.make_room(bytes.len(), None);
        self.insert(id, bytes);
        Ok(())
    }

    /// Open a warming batch; evictions are planned in bulk.
    pub fn begin_batch(&mut self) {
        self.batch_plan = Some(self.hot.iter().map(|(id, e)| (*id, e.touches)).collect());
    }

    /// Warm a page inside the open batch; evictions follow the batch plan.
    pub fn warm_in_batch(&mut self, id: u32, bytes: &[u8]) -> Result<()> {
        if self.hot.contains_key(&id) {
            return Ok(());
        }
        let plan = self.batch_plan.clone();
        self.make_room(bytes.len(), plan);
        self.insert(id, bytes);
        Ok(())
    }

    pub fn end_batch(&mut self) {
        self.batch_plan = None;
    }

    fn insert(&mut self, id: u32, bytes: &[u8]) {
        let boxed = bytes.to_vec().into_boxed_slice();
        let ptr = Box::into_raw(boxed) as *mut u8;
        self.hot_used += bytes.len();
        self.hot.insert(
            id,
            HotEntry {
                ptr,
                bytes: bytes.len(),
                touches: 1,
            },
        );
    }

    /// Free hot pages until `need` bytes fit. Batch evictions follow the
    /// batch's plan order.
    fn make_room(&mut self, need: usize, plan: Option<Vec<(u32, u32)>>) {
        if self.hot_used + need <= self.hot_budget {
            return;
        }
        let mut order: Vec<(u32, u32)> =
            plan.unwrap_or_else(|| self.hot.iter().map(|(id, e)| (*id, e.touches)).collect());
        order.sort_by_key(|(_, touches)| *touches);
        for (id, _) in order {
            if self.hot_used + need <= self.hot_budget {
                break;
            }
            if let Some(entry) = self.hot.remove(&id) {
                self.hot_used -= entry.bytes;
                unsafe {
                    drop(Box::from_raw(std::ptr::slice_from_raw_parts_mut(
                        entry.ptr,
                        entry.bytes,
                    )));
                }
            }
        }
    }

    pub fn resident(&self) -> usize {
        self.hot.len()
    }

    pub fn touches(&self, id: u32) -> u32 {
        self.hot.get(&id).map(|e| e.touches).unwrap_or(0)
    }
}

impl Drop for TieredCache {
    fn drop(&mut self) {
        for (_, entry) in self.hot.drain() {
            unsafe {
                drop(Box::from_raw(std::ptr::slice_from_raw_parts_mut(
                    entry.ptr,
                    entry.bytes,
                )));
            }
        }
    }
}

/// Driver ops for the tiered entry point.
mod tiered_ops {
    pub const WARM: u8 = 0x01;
    pub const FETCH: u8 = 0x02;
    pub const BEGIN_BATCH: u8 = 0x03;
    pub const WARM_BATCH: u8 = 0x04;
    pub const END_BATCH: u8 = 0x05;
    pub const READ: u8 = 0x06;
}

/// Driver for the tiered-cache entry point: warm/fetch/batch ops followed
/// by reads of outstanding views, in script order.
pub fn run_tiered_driver(raw: &[u8]) -> Result<u64> {
    let mut cache = TieredCache::new(1 << 9);
    let mut views: Vec<WarmView> = Vec::new();
    let mut checksum = 0u64;
    let mut r = Reader::new(raw);
    while !r.is_empty() {
        let op = r.u8("tiered op")?;
        match op {
            tiered_ops::WARM => {
                let id = r.varint("warm id")?;
                let bytes = r.len_slice("warm bytes")?;
                cache.warm(id, bytes)?;
            }
            tiered_ops::FETCH => {
                let id = r.varint("fetch id")?;
                if let Some(view) = cache.fetch(id) {
                    views.push(view);
                }
            }
            tiered_ops::BEGIN_BATCH => cache.begin_batch(),
            tiered_ops::WARM_BATCH => {
                let id = r.varint("batch warm id")?;
                let bytes = r.len_slice("batch warm bytes")?;
                cache.warm_in_batch(id, bytes)?;
            }
            tiered_ops::END_BATCH => cache.end_batch(),
            tiered_ops::READ => {
                let idx = r.varint("read view")? as usize;
                let view = views.get(idx).ok_or(Error::Session {
                    detail: format!("no view {idx}"),
                })?;
                for b in view.read() {
                    checksum = checksum.wrapping_mul(31).wrapping_add(b as u64);
                }
            }
            other => {
                return Err(Error::Session {
                    detail: format!("unknown op {other:#x}"),
                })
            }
        }
    }
    Ok(checksum)
}

/// Serialize a tiered driver script.
pub fn write_tiered_script(ops: &[(u8, u32, &[u8])]) -> Vec<u8> {
    let mut out = Vec::new();
    for (op, arg, bytes) in ops {
        out.push(*op);
        match *op {
            tiered_ops::WARM | tiered_ops::WARM_BATCH => {
                write_varint(&mut out, *arg);
                write_len_slice(&mut out, bytes);
            }
            tiered_ops::FETCH | tiered_ops::READ => {
                write_varint(&mut out, *arg);
            }
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn warm_fetch_promotes() {
        let mut c = TieredCache::new(1 << 12);
        c.warm(1, &[1u8; 32]).unwrap();
        let v = c.fetch(1).unwrap();
        assert_eq!(v.read(), vec![1u8; 32]);
        assert_eq!(c.touches(1), 2);
    }

    #[test]
    fn batch_warm_keeps_hot_pages() {
        let script = write_tiered_script(&[
            (tiered_ops::WARM, 1, &[1u8; 64]),
            (tiered_ops::WARM, 2, &[2u8; 64]),
            (tiered_ops::FETCH, 1, &[]),
            (tiered_ops::BEGIN_BATCH, 0, &[]),
            (tiered_ops::WARM_BATCH, 3, &[3u8; 64]),
            (tiered_ops::END_BATCH, 0, &[]),
            (tiered_ops::READ, 0, &[]),
        ]);
        assert!(run_tiered_driver(&script).is_ok());
    }

    #[test]
    fn plain_pressure_evicts_cold() {
        let mut c = TieredCache::new(128);
        c.warm(1, &[1u8; 96]).unwrap();
        c.warm(2, &[2u8; 96]).unwrap();
        assert!(c.fetch(2).is_some());
    }

    #[test]
    fn batch_plan_cleared_on_close() {
        let mut c = TieredCache::new(1 << 12);
        c.warm(1, &[1u8; 32]).unwrap();
        c.begin_batch();
        c.end_batch();
        assert!(c.batch_plan.is_none());
    }

    #[test]
    fn script_codec_round_trip_ops() {
        let ops: Vec<(u8, u32, &[u8])> = vec![
            (tiered_ops::WARM, 5, &[9u8; 8]),
            (tiered_ops::FETCH, 5, &[]),
            (tiered_ops::BEGIN_BATCH, 0, &[]),
            (tiered_ops::END_BATCH, 0, &[]),
            (tiered_ops::READ, 0, &[]),
        ];
        let raw = write_tiered_script(&ops);
        assert!(run_tiered_driver(&raw).is_ok());
    }
}
