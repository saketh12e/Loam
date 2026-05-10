//! Zone-map sidecar I/O: persist computed statistics next to a dataset so
//! reopens skip the statistics pass entirely.

use crate::bytes::{write_varint, Reader};
use crate::error::{Error, Result};
use crate::format::ZoneMap;

pub const SIDECAR_MAGIC: &[u8; 2] = b"ZM";

/// Serialize zone maps as a sidecar blob.
pub fn write_sidecar(zones: &[ZoneMap]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(SIDECAR_MAGIC);
    write_varint(&mut out, zones.len() as u32);
    for z in zones {
        out.extend_from_slice(&z.min.to_le_bytes());
        out.extend_from_slice(&z.max.to_le_bytes());
        out.extend_from_slice(&z.rows.to_le_bytes());
    }
    out
}

/// Parse a sidecar blob.
pub fn read_sidecar(raw: &[u8]) -> Result<Vec<ZoneMap>> {
    let mut r = Reader::new(raw);
    if r.take(2, "sidecar magic")? != SIDECAR_MAGIC {
        return Err(Error::BadMagic { what: "sidecar" });
    }
    let count = r.varint("sidecar zones")? as usize;
    if count == 0 || count > 4096 {
        return Err(Error::LengthMismatch {
            what: "sidecar zones",
        });
    }
    let mut zones = Vec::with_capacity(count);
    for _ in 0..count {
        zones.push(ZoneMap {
            min: r.i64("sidecar min")?,
            max: r.i64("sidecar max")?,
            rows: r.u32("sidecar rows")?,
        });
    }
    Ok(zones)
}

/// Merge two sidecar zone lists (union of ranges per index).
pub fn merge_sidecars(a: &[ZoneMap], b: &[ZoneMap]) -> Result<Vec<ZoneMap>> {
    if a.len() != b.len() {
        return Err(Error::LengthMismatch {
            what: "sidecar merge",
        });
    }
    Ok(a.iter()
        .zip(b.iter())
        .map(|(x, y)| ZoneMap {
            min: x.min.min(y.min),
            max: x.max.max(y.max),
            rows: x.rows + y.rows,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sidecar_round_trip() {
        let zones = vec![
            ZoneMap {
                min: 0,
                max: 10,
                rows: 10,
            },
            ZoneMap {
                min: 20,
                max: 30,
                rows: 5,
            },
        ];
        let raw = write_sidecar(&zones);
        assert_eq!(read_sidecar(&raw).unwrap(), zones);
    }

    #[test]
    fn merges_ranges() {
        let a = vec![ZoneMap {
            min: 0,
            max: 5,
            rows: 5,
        }];
        let b = vec![ZoneMap {
            min: 3,
            max: 9,
            rows: 4,
        }];
        let merged = merge_sidecars(&a, &b).unwrap();
        assert_eq!(merged[0].min, 0);
        assert_eq!(merged[0].max, 9);
        assert_eq!(merged[0].rows, 9);
    }
}
