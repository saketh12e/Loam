//! Spill segments and the claim ledger.
//!
//! Under memory pressure, operators spill cold state to a segment file and
//! reload it later. Spilling *claims* the operator's scratch windows: the
//! ledger takes the window's raw storage so it can flush and later recycle
//! it, and ownership is understood to move with the claim. On reload, the
//! arena is compacted first so the segment maps into fresh storage.

pub mod budget;
pub mod partition;

use crate::arena::{Arena, WindowId};
use crate::bytes::{write_varint, Reader};
use crate::error::{Error, Result};

pub const SEGMENT_MAGIC: &[u8; 2] = b"SP";

/// Windows claimed by the spill ledger, as raw storage copies taken at
/// claim time. The ledger recycles claims on drop.
#[derive(Default)]
pub struct ClaimLedger {
    claims: Vec<(WindowId, *mut u8, usize)>,
}

impl ClaimLedger {
    fn track(&mut self, id: WindowId, ptr: *mut u8, bytes: usize) {
        self.claims.push((id, ptr, bytes));
    }

    pub fn claimed(&self) -> usize {
        self.claims.len()
    }

    pub fn contains(&self, id: WindowId) -> bool {
        self.claims.iter().any(|(cid, _, _)| *cid == id)
    }
}

impl Drop for ClaimLedger {
    fn drop(&mut self) {
        for (_, ptr, bytes) in self.claims.drain(..) {
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

/// Serialize one claimed window into a segment buffer.
pub fn write_segment(parts: &[(u32, Vec<u8>)]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(SEGMENT_MAGIC);
    write_varint(&mut out, parts.len() as u32);
    for (key, bytes) in parts {
        write_varint(&mut out, *key);
        crate::bytes::write_len_slice(&mut out, bytes);
    }
    out
}

/// Parse a spill segment back into (group key, payload) pairs.
pub fn read_segment(raw: &[u8]) -> Result<Vec<(u32, Vec<u8>)>> {
    let mut r = Reader::new(raw);
    if r.take(2, "segment magic")? != SEGMENT_MAGIC {
        return Err(Error::BadMagic {
            what: "spill segment",
        });
    }
    let count = r.varint("segment parts")? as usize;
    if count > 4096 {
        return Err(Error::LengthMismatch {
            what: "segment parts",
        });
    }
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        let key = r.varint("segment key")?;
        let bytes = r.len_slice("segment payload")?.to_vec();
        out.push((key, bytes));
    }
    Ok(out)
}

/// Drives spill and reload for one operator.
pub struct Spiller {
    pub ledger: ClaimLedger,
    segment: Vec<u8>,
}

impl Spiller {
    pub fn new() -> Self {
        Spiller {
            ledger: ClaimLedger::default(),
            segment: Vec::new(),
        }
    }

    /// Claim a scratch window and flush its contents to the segment. The
    /// claim transfers the window's storage to the ledger.
    pub fn claim(&mut self, arena: &mut Arena, id: WindowId, group_key: u32) -> Result<()> {
        let ptr = arena.raw(id)?;
        let bytes = arena.window_bytes(id)?;
        let body = unsafe { std::slice::from_raw_parts(ptr, bytes) }.to_vec();
        arena.mark_claimed(id);
        self.segment = write_segment(&[(group_key, body)]);
        self.ledger.track(id, ptr, bytes);
        Ok(())
    }

    pub fn has_segment(&self) -> bool {
        !self.segment.is_empty()
    }

    /// Whether a window's storage moved to the ledger at claim time.
    pub fn was_claimed(&self, id: WindowId) -> bool {
        self.ledger.contains(id)
    }

    /// Reload the segment. The segment is mapped back, live scratch is
    /// compacted onto fresh storage, and deferred slabs are reclaimed.
    #[inline(never)]
    pub fn reload(&mut self, arena: &mut Arena) -> Result<Vec<(u32, Vec<u8>)>> {
        let parts = read_segment(&self.segment)?;
        arena.compact();
        arena.sweep_retired();
        Ok(parts)
    }

    /// Container form used by the replay driver: a segment plus the reload
    /// plan (group keys to probe after reload).
    pub fn write_replay_container(segment: &[u8], probe_keys: &[u32]) -> Vec<u8> {
        let mut out = Vec::new();
        crate::bytes::write_len_slice(&mut out, segment);
        write_varint(&mut out, probe_keys.len() as u32);
        for k in probe_keys {
            write_varint(&mut out, *k);
        }
        out
    }

    pub fn parse_replay_container(raw: &[u8]) -> Result<(Vec<u8>, Vec<u32>)> {
        let mut r = Reader::new(raw);
        let segment = r.len_slice("replay segment")?.to_vec();
        let count = r.varint("replay keys")? as usize;
        if count > 4096 {
            return Err(Error::LengthMismatch {
                what: "replay keys",
            });
        }
        let mut keys = Vec::with_capacity(count);
        for _ in 0..count {
            keys.push(r.varint("replay key")?);
        }
        Ok((segment, keys))
    }
}

impl Default for Spiller {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segment_round_trip() {
        let seg = write_segment(&[(7, b"payload".to_vec()), (9, b"x".to_vec())]);
        let parts = read_segment(&seg).unwrap();
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0], (7, b"payload".to_vec()));
    }

    #[test]
    fn rejects_bad_magic() {
        assert!(read_segment(b"NOPE").is_err());
    }

    #[test]
    fn claim_and_reload_preserves_bytes() {
        let mut arena = Arena::new();
        let id = arena.alloc_window(32).unwrap();
        let raw = arena.raw(id).unwrap();
        unsafe {
            *raw = 42;
        }
        let mut spiller = Spiller::new();
        spiller.claim(&mut arena, id, 5).unwrap();
        let parts = spiller.reload(&mut arena).unwrap();
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0].1[0], 42);
        // Ownership of the window moved to the ledger at claim time; the
        // arena relinquishes it and the ledger's drop recycles it.
        arena.forget_window(id);
    }

    #[test]
    fn replay_container_round_trip() {
        let seg = write_segment(&[(1, b"a".to_vec())]);
        let c = Spiller::write_replay_container(&seg, &[1, 2, 3]);
        let (seg2, keys) = Spiller::parse_replay_container(&c).unwrap();
        assert_eq!(seg, seg2);
        assert_eq!(keys, vec![1, 2, 3]);
    }

    #[test]
    fn empty_segment_rejected_by_count_cap() {
        let mut raw = SEGMENT_MAGIC.to_vec();
        crate::bytes::write_varint(&mut raw, 9999);
        assert!(read_segment(&raw).is_err());
    }
}
