//! Runtime counters for engine operations.

#[derive(Debug, Default, Clone, Copy)]
pub struct Metrics {
    pub files_opened: u64,
    pub pages_decoded: u64,
    pub rows_scanned: u64,
    pub spills: u64,
    pub evictions: u64,
}

impl Metrics {
    pub fn snapshot(&self) -> Self {
        *self
    }
}

