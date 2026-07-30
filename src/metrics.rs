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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_copies() {
        let m = Metrics {
            pages_decoded: 7,
            ..Default::default()
        };
        let s = m.snapshot();
        assert_eq!(s.pages_decoded, 7);
    }

    #[test]
    fn counters_accumulate() {
        let mut m = Metrics::default();
        m.files_opened += 2;
        m.pages_decoded += 40;
        m.spills += 1;
        let s = m.snapshot();
        assert_eq!(s.files_opened, 2);
        assert_eq!(s.pages_decoded, 40);
        assert_eq!(s.spills, 1);
    }
}
