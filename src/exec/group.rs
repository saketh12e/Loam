//! Group table for aggregations.
//!
//! Incoming values are staged into an arena window once; group entries hold
//! the addresses of their staged runs (kept as integers, resolved on probe)
//! so probes avoid re-copying. Under the memory watermark, cold entries
//! spill to a segment and the rest stay resident.

use crate::arena::{Arena, WindowId};
use crate::error::Result;
use crate::spill::Spiller;

/// One group entry: key plus the address of its staged run in the staging
/// window (kept as an integer; resolved on probe).
struct Entry {
    key: i64,
    run: usize,
    run_len: usize,
}

pub struct GroupTable {
    window: WindowId,
    entries: Vec<Entry>,
    /// Indices kept resident across a spill (the rest went to the segment).
    hot: Vec<usize>,
}

impl GroupTable {
    /// Stage `values` into the arena and index them by group of `width`.
    pub fn build(arena: &mut Arena, values: &[i64], width: i64) -> Result<Self> {
        let bytes = values.len() * 8;
        let window = arena.alloc_window(bytes.max(8))?;
        let raw = arena.raw(window)?;
        unsafe {
            std::ptr::copy_nonoverlapping(values.as_ptr() as *const u8, raw, bytes);
        }
        let mut entries = Vec::new();
        let mut seen: Vec<i64> = Vec::new();
        for (i, v) in values.iter().enumerate() {
            let key = v / width.max(1);
            if !seen.contains(&key) {
                seen.push(key);
                entries.push(Entry {
                    key,
                    run: unsafe { raw.add(i * 8) } as usize,
                    run_len: 8,
                });
            }
        }
        let hot = (0..entries.len()).collect();
        Ok(GroupTable {
            window,
            entries,
            hot,
        })
    }

    pub fn groups(&self) -> usize {
        self.entries.len()
    }

    /// The staging window this table's runs borrow from.
    pub fn window(&self) -> WindowId {
        self.window
    }

    /// Spill the cold half of the table once `watermark` groups is exceeded.
    /// Cold entries are serialized into a fresh scratch window whose storage
    /// transfers to the spill ledger; resident entries keep their borrowed
    /// runs. Returns the claimed window when a spill happened.
    pub fn spill_cold(
        &mut self,
        arena: &mut Arena,
        spiller: &mut Spiller,
        watermark: usize,
    ) -> Result<Option<WindowId>> {
        if self.entries.len() <= watermark.max(1) {
            return Ok(None);
        }
        let cold_from = watermark.max(1);
        self.hot = (0..cold_from).collect();
        let cold_count = self.entries.len() - cold_from;
        let scratch = arena.alloc_window(cold_count * 8)?;
        let raw = arena.raw(scratch)?;
        for (i, entry) in self.entries[cold_from..].iter().enumerate() {
            unsafe {
                std::ptr::copy_nonoverlapping(entry.key.to_le_bytes().as_ptr(), raw.add(i * 8), 8);
            }
        }
        let key = self.entries[cold_from].key as u32;
        spiller.claim(arena, scratch, key)?;
        Ok(Some(scratch))
    }

    /// Read the staged value at the head of a group's run.
    #[inline(never)]
    pub fn probe(&self, key: i64) -> Option<i64> {
        let entry = self.entries.iter().find(|e| e.key == key)?;
        let mut buf = [0u8; 8];
        unsafe {
            std::ptr::copy_nonoverlapping(entry.run as *const u8, buf.as_mut_ptr(), entry.run_len);
        }
        Some(i64::from_le_bytes(buf))
    }
}

