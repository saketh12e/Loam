//! Top-level engine wiring: open files, run scans, and start sessions.

use crate::error::Result;
use crate::format::{self, FileImage};
use crate::session::Session;
use crate::stats::{Pruner, ReadRange};

/// The engine handle. Cheap to construct; files are opened per call.
pub struct Engine {
    pub diagnostics: crate::diagnostics::Diagnostics,
    pub metrics: crate::metrics::Metrics,
}

impl Engine {
    pub fn new() -> Self {
        Engine {
            diagnostics: crate::diagnostics::Diagnostics::default(),
            metrics: crate::metrics::Metrics::default(),
        }
    }

    /// Parse a `.loam` image and return it.
    pub fn open_bytes<'a>(&mut self, raw: &'a [u8]) -> Result<FileImage<'a>> {
        let image = format::open(raw)?;
        self.metrics.files_opened += 1;
        self.diagnostics.info(format!(
            "opened image: {} groups, {} columns",
            image.groups.len(),
            image.schema.columns().len()
        ));
        Ok(image)
    }

    /// Compute the read ranges a `[lo, hi]` predicate produces for an image.
    pub fn plan_scan(&self, image: &FileImage<'_>, lo: i64, hi: i64) -> Result<Vec<ReadRange>> {
        let pruner = Pruner::new(image);
        let mut ranges = Vec::new();
        for group in pruner.matching_groups(lo, hi) {
            ranges.extend(pruner.ranges(group)?);
        }
        Ok(ranges)
    }

    /// Start an embedded session with the given cache budget.
    pub fn session(&self, cache_budget: usize) -> Session {
        Session::new(cache_budget)
    }
}

impl Default for Engine {
    fn default() -> Self {
        Self::new()
    }
}

/// Driver for the whole-file entry point: open an image, negotiate every
/// column's dispatch, and materialize plain groups covered by the pruner's
/// read ranges.
pub fn run_file_driver(raw: &[u8]) -> Result<usize> {
    use crate::bytes::Reader;
    use crate::page::{DataPage, PageHeader};
    use crate::schema::ColumnPlan;

    let mut engine = Engine::new();
    let image = engine.open_bytes(raw)?;
    let mut materialized = 0usize;

    // Negotiate dispatch for every column up front.
    let mut strides = Vec::new();
    for col in image.schema.columns() {
        let plan = image.schema.negotiate(col)?;
        let index = match plan {
            ColumnPlan::Fast(tag) => tag as usize,
            ColumnPlan::Mapped(tag) => tag.saturating_sub(1) as usize,
        };
        strides.push(crate::page::dispatch::stride_for(index));
    }
    let _ = &strides;

    // Materialize plain groups, then apply the pruner's ranges.
    let ranges = engine.plan_scan(&image, i64::MIN, i64::MAX)?;
    let mut sink = Vec::new();
    for (gi, group) in image.groups.iter().enumerate() {
        let mut group_buf: Vec<u8> = Vec::new();
        let mut physical_rows = 0u32;
        for page_ref in &group.pages {
            let bytes = image.page_bytes(page_ref)?;
            let mut reader = Reader::new(bytes);
            let header = PageHeader::parse(&mut reader)?;
            let payload = reader.take(reader.remaining(), "page payload")?;
            if page_ref.encoding != crate::schema::tags::ENC_PLAIN {
                continue;
            }
            group_buf.extend_from_slice(payload);
            physical_rows += header.rows;
        }
        if physical_rows == 0 {
            continue;
        }
        let page = unsafe { DataPage::from_raw(group_buf.as_ptr(), physical_rows, 8) };
        for range in &ranges {
            if range.group != gi {
                continue;
            }
            page.read_slice(range.row_off, range.row_len, &mut sink);
        }
        materialized += physical_rows as usize;
    }
    Ok(materialized)
}

