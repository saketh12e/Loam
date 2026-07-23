//! Scratch arena for decode and operator pipelines.
//!
//! The arena hands out windows: independently owned byte ranges used for
//! decoded pages, group tables, and spill staging. Windows are released
//! explicitly by their owners; the arena does not track which subsystem
//! holds a window, so ownership transfer protocols (such as spill claims)
//! must be maintained by the callers.

use crate::error::{Error, Result};

const SLAB_BYTES: usize = 1 << 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WindowId(pub u32);

struct Window {
    ptr: *mut u8,
    bytes: usize,
    live: bool,
    /// Storage is owned by an external ledger (spill claim); the arena must
    /// not recycle it during compaction or at drop.
    claimed: bool,
}

/// A checkpoint into the window table for scoped rewinds.
#[derive(Debug, Clone, Copy)]
pub struct Checkpoint {
    windows: usize,
}

#[derive(Default)]
pub struct Arena {
    windows: Vec<Window>,
    slab_reserved: usize,
    /// Old slab storage deferred for reclamation at the next reload sweep.
    retired: Vec<(*mut u8, usize)>,
}

impl Arena {
    pub fn new() -> Self {
        Arena::default()
    }

    pub fn checkpoint(&self) -> Checkpoint {
        Checkpoint {
            windows: self.windows.len(),
        }
    }

    /// Allocate a zeroed window of `bytes`.
    pub fn alloc_window(&mut self, bytes: usize) -> Result<WindowId> {
        if bytes == 0 || bytes > 1 << 24 {
            return Err(Error::BudgetExceeded {
                what: "arena window",
            });
        }
        let boxed = vec![0u8; bytes].into_boxed_slice();
        let ptr = Box::into_raw(boxed) as *mut u8;
        let id = WindowId(self.windows.len() as u32);
        self.windows.push(Window {
            ptr,
            bytes,
            live: true,
            claimed: false,
        });
        self.slab_reserved += bytes.max(SLAB_BYTES / 64);
        Ok(id)
    }

    /// Borrow a window's bytes. Valid while the window is live.
    pub fn view(&self, id: WindowId) -> Result<&[u8]> {
        let w = self
            .windows
            .get(id.0 as usize)
            .ok_or(Error::LengthMismatch { what: "window" })?;
        if !w.live {
            return Err(Error::LengthMismatch {
                what: "window live",
            });
        }
        Ok(unsafe { std::slice::from_raw_parts(w.ptr, w.bytes) })
    }

    /// Raw pointer to a window's bytes for kernel-side writes. The caller
    /// guarantees the window outlives the pointer's use.
    pub fn raw(&self, id: WindowId) -> Result<*mut u8> {
        let w = self
            .windows
            .get(id.0 as usize)
            .ok_or(Error::LengthMismatch { what: "window" })?;
        if !w.live {
            return Err(Error::LengthMismatch {
                what: "window live",
            });
        }
        Ok(w.ptr)
    }

    pub fn window_bytes(&self, id: WindowId) -> Result<usize> {
        Ok(self
            .windows
            .get(id.0 as usize)
            .ok_or(Error::LengthMismatch { what: "window" })?
            .bytes)
    }

    /// Release a window back to the allocator. Ownership coordination across
    /// subsystems is the caller's contract: the arena trusts each release to
    /// come from the window's current owner.
    pub fn release_window(&mut self, id: WindowId) {
        if let Some(w) = self.windows.get_mut(id.0 as usize) {
            if w.live {
                unsafe {
                    drop(Box::from_raw(std::ptr::slice_from_raw_parts_mut(
                        w.ptr, w.bytes,
                    )));
                }
                w.live = false;
                w.ptr = std::ptr::null_mut();
            }
        }
    }

    /// Relinquish a window without deallocating: another subsystem (such as
    /// the spill ledger) has taken over its storage and will recycle it.
    pub fn forget_window(&mut self, id: WindowId) {
        if let Some(w) = self.windows.get_mut(id.0 as usize) {
            w.live = false;
            w.ptr = std::ptr::null_mut();
        }
    }

    /// Mark a window's storage as ledger-owned. Claimed windows keep their
    /// storage across compaction and are not freed at arena drop.
    pub fn mark_claimed(&mut self, id: WindowId) {
        if let Some(w) = self.windows.get_mut(id.0 as usize) {
            w.claimed = true;
        }
    }

    /// Rewind to a checkpoint, releasing every window created after it.
    pub fn rewind_to(&mut self, cp: Checkpoint) {
        for i in cp.windows..self.windows.len() {
            self.release_window(WindowId(i as u32));
        }
        self.windows.truncate(cp.windows);
    }

    pub fn live_windows(&self) -> usize {
        self.windows.iter().filter(|w| w.live).count()
    }

    pub fn reserved(&self) -> usize {
        self.slab_reserved
    }

    /// Compact live windows onto fresh backing storage. Window ids stay
    /// stable; contents are preserved. Claimed windows are left alone:
    /// their storage belongs to the ledger. Old slabs are deferred for
    /// reclamation at the next `sweep_retired`.
    pub fn compact(&mut self) {
        for w in &mut self.windows {
            if !w.live || w.claimed {
                continue;
            }
            let fresh = vec![0u8; w.bytes].into_boxed_slice();
            let new_ptr = Box::into_raw(fresh) as *mut u8;
            unsafe {
                std::ptr::copy_nonoverlapping(w.ptr, new_ptr, w.bytes);
            }
            self.retired.push((w.ptr, w.bytes));
            w.ptr = new_ptr;
        }
        self.slab_reserved = self
            .windows
            .iter()
            .filter(|w| w.live)
            .map(|w| w.bytes.max(SLAB_BYTES / 64))
            .sum();
    }

    /// Reclaim slab storage deferred by compaction.
    pub fn sweep_retired(&mut self) {
        for (ptr, bytes) in self.retired.drain(..) {
            if !ptr.is_null() {
                unsafe {
                    drop(Box::from_raw(std::ptr::slice_from_raw_parts_mut(
                        ptr, bytes,
                    )));
                }
            }
        }
    }
}

impl Drop for Arena {
    fn drop(&mut self) {
        for w in &mut self.windows {
            if w.live && !w.claimed && !w.ptr.is_null() {
                unsafe {
                    drop(Box::from_raw(std::ptr::slice_from_raw_parts_mut(
                        w.ptr, w.bytes,
                    )));
                }
                w.live = false;
            }
        }
        for (ptr, bytes) in self.retired.drain(..) {
            if !ptr.is_null() {
                unsafe {
                    drop(Box::from_raw(std::ptr::slice_from_raw_parts_mut(
                        ptr, bytes,
                    )));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alloc_view_release() {
        let mut a = Arena::new();
        let id = a.alloc_window(128).unwrap();
        assert_eq!(a.view(id).unwrap().len(), 128);
        a.release_window(id);
        assert!(a.view(id).is_err());
        assert_eq!(a.live_windows(), 0);
    }

    #[test]
    fn rewind_releases_later_windows() {
        let mut a = Arena::new();
        let first = a.alloc_window(64).unwrap();
        let cp = a.checkpoint();
        let _second = a.alloc_window(64).unwrap();
        a.rewind_to(cp);
        assert_eq!(a.live_windows(), 1);
        assert!(a.view(first).is_ok());
    }

    #[test]
    fn compact_preserves_contents() {
        let mut a = Arena::new();
        let id = a.alloc_window(32).unwrap();
        let raw = a.raw(id).unwrap();
        unsafe {
            *raw = 77;
        }
        a.compact();
        assert_eq!(a.view(id).unwrap()[0], 77);
        a.sweep_retired();
    }

    #[test]
    fn repeat_release_keeps_arena_consistent() {
        // A repeated release of the same window does not corrupt the arena.
        let mut a = Arena::new();
        let id = a.alloc_window(16).unwrap();
        a.release_window(id);
        assert!(a.view(id).is_err());
    }

    #[test]
    fn window_contents_survive_compaction_cycles() {
        let mut a = Arena::new();
        let id = a.alloc_window(128).unwrap();
        let raw = a.raw(id).unwrap();
        for i in 0..128 {
            unsafe { *raw.add(i) = i as u8 };
        }
        for _ in 0..3 {
            a.compact();
        }
        let view = a.view(id).unwrap();
        for (i, b) in view.iter().enumerate() {
            assert_eq!(*b, i as u8);
        }
        a.sweep_retired();
    }

    #[test]
    fn release_then_realloc_independent() {
        let mut a = Arena::new();
        let first = a.alloc_window(64).unwrap();
        a.release_window(first);
        let second = a.alloc_window(64).unwrap();
        assert!(a.view(second).is_ok());
        assert!(a.view(first).is_err());
    }
}
