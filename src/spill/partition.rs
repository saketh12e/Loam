//! Partitioned spill segments: one segment per group range so reloads can
//! stream only the partitions a query needs.

use crate::bytes::{write_len_slice, write_varint, Reader};
use crate::error::{Error, Result};

pub const PART_MAGIC: &[u8; 2] = b"PP";

/// Write a partitioned segment: (partition id, payload) pairs.
pub fn write_partitions(parts: &[(u32, Vec<u8>)]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(PART_MAGIC);
    write_varint(&mut out, parts.len() as u32);
    for (id, payload) in parts {
        write_varint(&mut out, *id);
        write_len_slice(&mut out, payload);
    }
    out
}

/// Read back only the requested partitions.
pub fn read_partitions(raw: &[u8], wanted: &[u32]) -> Result<Vec<(u32, Vec<u8>)>> {
    let mut r = Reader::new(raw);
    if r.take(2, "partition magic")? != PART_MAGIC {
        return Err(Error::BadMagic {
            what: "partition segment",
        });
    }
    let count = r.varint("partition count")? as usize;
    if count > 4096 {
        return Err(Error::LengthMismatch {
            what: "partition count",
        });
    }
    let mut out = Vec::new();
    for _ in 0..count {
        let id = r.varint("partition id")?;
        let payload = r.len_slice("partition payload")?;
        if wanted.contains(&id) {
            out.push((id, payload.to_vec()));
        }
    }
    Ok(out)
}

/// Partition assignment for a group key (stable across spills and reloads).
pub fn partition_of(key: i64, partitions: u32) -> u32 {
    (key as u64 % partitions.max(1) as u64) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selective_reload() {
        let seg = write_partitions(&[(0, b"a".to_vec()), (1, b"b".to_vec()), (2, b"c".to_vec())]);
        let got = read_partitions(&seg, &[1]).unwrap();
        assert_eq!(got, vec![(1, b"b".to_vec())]);
    }

    #[test]
    fn partition_assignment_is_stable() {
        assert_eq!(partition_of(7, 4), 3);
        assert_eq!(partition_of(8, 4), 0);
    }
}
