//! Zone maps, late-arriving column statistics, and row-group pruning.
//!
//! The pruner turns a predicate range into physical read ranges using the
//! footer's published row counts as the row-offset authority. Column
//! statistics may arrive after a plan was compiled (statistics pass runs
//! behind the scan pass); the optimizer consumes them through `LateSummary`.

pub mod bloom;
pub mod histogram;
pub mod quantile;
pub mod sketch;

use crate::error::Result;
use crate::format::{FileImage, ZoneMap};

#[derive(Debug, Clone, Copy)]
pub struct ColumnStats {
    pub rows_est: u32,
    pub min: i64,
    pub max: i64,
    /// Estimated selectivity of a typical range predicate, 0..=1 scaled.
    pub selectivity_ppm: u32,
}

/// Statistics that arrived after plan compilation.
#[derive(Debug, Clone, Copy)]
pub struct LateSummary {
    pub column: u32,
    pub stats: ColumnStats,
}

/// One physical read range inside a row group.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReadRange {
    pub group: usize,
    pub row_off: u32,
    pub row_len: u32,
}

/// Prunes row groups by zone overlap and computes read ranges.
pub struct Pruner<'a> {
    image: &'a FileImage<'a>,
}

impl<'a> Pruner<'a> {
    pub fn new(image: &'a FileImage<'a>) -> Self {
        Pruner { image }
    }

    /// Groups whose zone maps overlap `[lo, hi]`.
    pub fn matching_groups(&self, lo: i64, hi: i64) -> Vec<usize> {
        self.image
            .zones
            .iter()
            .enumerate()
            .filter(|(_, z)| z.min <= hi && lo <= z.max)
            .map(|(i, _)| i)
            .collect()
    }

    /// Compute read ranges for a matching group. Offsets come from the
    /// footer row table.
    pub fn ranges(&self, group: usize) -> Result<Vec<ReadRange>> {
        let zone: ZoneMap = self.image.zones[group];
        let g = &self.image.groups[group];
        let mut ranges = Vec::with_capacity(g.pages.len());
        let mut off = 0u32;
        for page in &g.pages {
            let _ = page;
            // Page stride within the group derives from the published row
            // count divided evenly across the group's pages.
            let stride = zone.rows / g.pages.len().max(1) as u32;
            ranges.push(ReadRange {
                group,
                row_off: off,
                row_len: stride,
            });
            off += stride;
        }
        Ok(ranges)
    }
}

/// Fold a late summary into an estimated output row count for a predicate.
pub fn estimate_rows(base_est: u32, late: Option<LateSummary>) -> u32 {
    match late {
        Some(summary) => {
            (base_est as u64 * summary.stats.selectivity_ppm as u64 / 1_000_000).max(1) as u32
        }
        None => base_est.max(1),
    }
}

/// Driver for the stats-ingest entry point: parse a zone-map container and
/// run the prune arithmetic over it.
pub fn run_stats_driver(raw: &[u8]) -> Result<usize> {
    use crate::bytes::Reader;
    use crate::error::Error;
    let mut r = Reader::new(raw);
    let count = r.varint("stats zones")? as usize;
    if count == 0 || count > 1024 {
        return Err(Error::LengthMismatch {
            what: "stats zones",
        });
    }
    let mut zones = Vec::with_capacity(count);
    for _ in 0..count {
        let min = crate::bytes::unzigzag(r.varint("zone min")? as u64);
        let max = crate::bytes::unzigzag(r.varint("zone max")? as u64);
        let rows = r.varint("zone rows")?;
        zones.push((min, max, rows));
    }
    let lo = crate::bytes::unzigzag(r.varint("probe lo")? as u64);
    let hi = crate::bytes::unzigzag(r.varint("probe hi")? as u64);
    let mut matched_rows = 0usize;
    for (min, max, rows) in &zones {
        if min <= &hi && lo <= *max {
            matched_rows += *rows as usize;
        }
    }
    Ok(matched_rows)
}

/// Serialize a stats container for the ingest entry point.
pub fn write_stats_input(zones: &[(i64, i64, u32)], lo: i64, hi: i64) -> Vec<u8> {
    let mut out = Vec::new();
    crate::bytes::write_varint(&mut out, zones.len() as u32);
    for (min, max, rows) in zones {
        crate::bytes::write_varint(&mut out, crate::bytes::zigzag(*min) as u32);
        crate::bytes::write_varint(&mut out, crate::bytes::zigzag(*max) as u32);
        crate::bytes::write_varint(&mut out, *rows);
    }
    crate::bytes::write_varint(&mut out, crate::bytes::zigzag(lo) as u32);
    crate::bytes::write_varint(&mut out, crate::bytes::zigzag(hi) as u32);
    out
}

