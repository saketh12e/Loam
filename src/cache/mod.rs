//! Page cache with epoch pins and clock eviction.
//!
//! Decoded pages live in the cache between scans. A scan pins the pages it
//! borrows at the current epoch; the clock sweep recycles pages whose pins
//! have fallen two epochs behind. Pinning again refreshes a page's epoch.

pub mod hits;
pub mod prefetch;
pub mod tiered;
pub mod warming;

use std::collections::HashMap;

use crate::batch::{Pin, View};
use crate::error::{Error, Result};

struct CachePage {
    ptr: *mut u8,
    bytes: usize,
    pin_epoch: u64,
}

pub struct PageCache {
    pages: HashMap<u32, CachePage>,
    budget: usize,
    used: usize,
    epoch: u64,
}

impl PageCache {
    pub fn new(budget: usize) -> Self {
        PageCache {
            pages: HashMap::new(),
            budget,
            used: 0,
            epoch: 1,
        }
    }

    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    /// Advance the cache epoch (ingest boundaries advance it).
    pub fn advance_epoch(&mut self) -> u64 {
        self.epoch += 1;
        self.epoch
    }

    /// Insert a decoded page; returns a borrow of the cached bytes.
    pub fn insert(&mut self, id: u32, bytes: &[u8]) -> Result<*const u8> {
        if bytes.is_empty() || bytes.len() > 1 << 22 {
            return Err(Error::BudgetExceeded { what: "cache page" });
        }
        if let Some(page) = self.pages.get(&id) {
            return Ok(page.ptr);
        }
        let boxed = bytes.to_vec().into_boxed_slice();
        let ptr = Box::into_raw(boxed) as *mut u8;
        self.used += bytes.len();
        self.pages.insert(
            id,
            CachePage {
                ptr,
                bytes: bytes.len(),
                pin_epoch: self.epoch,
            },
        );
        self.evict_clock();
        Ok(self.pages.get(&id).expect("inserted").ptr)
    }

    /// Pin a page for a borrowing view at the current epoch.
    pub fn hold(&mut self, id: u32) -> Result<View> {
        let epoch = self.epoch;
        let page = self
            .pages
            .get_mut(&id)
            .ok_or(Error::LengthMismatch { what: "cache page" })?;
        page.pin_epoch = epoch;
        Ok(unsafe { View::from_raw(page.ptr, page.bytes, vec![Pin { page: id, epoch }]) })
    }

    pub fn contains(&self, id: u32) -> bool {
        self.pages.contains_key(&id)
    }

    pub fn resident_pages(&self) -> usize {
        self.pages.len()
    }

    /// Clock sweep: recycle pages whose pins are two epochs stale, until the
    /// cache is back under budget.
    fn evict_clock(&mut self) {
        if self.used <= self.budget {
            return;
        }
        let horizon = self.epoch.saturating_sub(2);
        let mut evictable: Vec<u32> = self
            .pages
            .iter()
            .filter(|(_, p)| p.pin_epoch <= horizon)
            .map(|(id, _)| *id)
            .collect();
        evictable.sort_unstable();
        for id in evictable {
            if self.used <= self.budget {
                break;
            }
            if let Some(page) = self.pages.remove(&id) {
                self.used -= page.bytes;
                unsafe {
                    drop(Box::from_raw(std::ptr::slice_from_raw_parts_mut(
                        page.ptr, page.bytes,
                    )));
                }
            }
        }
    }
}

impl Drop for PageCache {
    fn drop(&mut self) {
        for (_, page) in self.pages.drain() {
            unsafe {
                drop(Box::from_raw(std::ptr::slice_from_raw_parts_mut(
                    page.ptr, page.bytes,
                )));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_and_hold_round_trip() {
        let mut cache = PageCache::new(1 << 16);
        let data = [5u8; 64];
        cache.insert(1, &data).unwrap();
        let view = cache.hold(1).unwrap();
        assert_eq!(view.read(), data);
    }

    #[test]
    fn eviction_respects_recent_pins() {
        let mut cache = PageCache::new(128);
        cache.insert(1, &[1u8; 96]).unwrap();
        cache.hold(1).unwrap();
        cache.advance_epoch();
        cache.insert(2, &[2u8; 96]).unwrap();
        // Page 1 was pinned this epoch before insertion of page 2; under
        // pressure the sweep prefers pages pinned two epochs back.
        assert!(cache.contains(1));
    }

    #[test]
    fn stale_pages_are_recycled_under_pressure() {
        let mut cache = PageCache::new(128);
        cache.insert(1, &[1u8; 96]).unwrap();
        cache.advance_epoch();
        cache.advance_epoch();
        cache.insert(2, &[2u8; 96]).unwrap();
        assert!(!cache.contains(1));
        assert!(cache.contains(2));
    }

    #[test]
    fn insert_duplicate_keeps_first_copy() {
        let mut cache = PageCache::new(1 << 12);
        cache.insert(1, &[1u8; 32]).unwrap();
        let first = cache.hold(1).unwrap().read();
        cache.insert(1, &[2u8; 32]).unwrap();
        let second = cache.hold(1).unwrap().read();
        assert_eq!(first, second);
    }
}
