//! Session checkpoints: capture and restore the engine-visible session
//! counters so long workflows can resume after process restarts.

use crate::bytes::{write_varint, Reader};
use crate::error::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionCheckpoint {
    pub cache_epoch: u64,
    pub held_views: u32,
    pub read_checksum: u64,
    pub plans_cached: u32,
}

/// Serialize a checkpoint.
pub fn write_checkpoint(cp: &SessionCheckpoint) -> Vec<u8> {
    let mut out = Vec::new();
    write_varint(&mut out, cp.cache_epoch as u32);
    write_varint(&mut out, cp.held_views);
    out.extend_from_slice(&cp.read_checksum.to_le_bytes());
    write_varint(&mut out, cp.plans_cached);
    out
}

/// Parse a checkpoint.
pub fn parse_checkpoint(raw: &[u8]) -> Result<SessionCheckpoint> {
    let mut r = Reader::new(raw);
    let cache_epoch = r.varint("checkpoint epoch")? as u64;
    let held_views = r.varint("checkpoint views")?;
    let read_checksum = r.u64("checkpoint checksum")?;
    let plans_cached = r.varint("checkpoint plans")?;
    if !r.is_empty() {
        return Err(Error::LengthMismatch {
            what: "checkpoint trailer",
        });
    }
    Ok(SessionCheckpoint {
        cache_epoch,
        held_views,
        read_checksum,
        plans_cached,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let cp = SessionCheckpoint {
            cache_epoch: 9,
            held_views: 3,
            read_checksum: 0xdead_beef,
            plans_cached: 2,
        };
        assert_eq!(parse_checkpoint(&write_checkpoint(&cp)).unwrap(), cp);
    }

    #[test]
    fn rejects_trailer() {
        let mut raw = write_checkpoint(&SessionCheckpoint {
            cache_epoch: 1,
            held_views: 0,
            read_checksum: 0,
            plans_cached: 0,
        });
        raw.push(0);
        assert!(parse_checkpoint(&raw).is_err());
    }
}
