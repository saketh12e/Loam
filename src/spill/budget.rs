//! Spill budget accounting: tracks scratch usage against the operator
//! watermark and decides when a spill is due.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Budget {
    pub watermark: usize,
    pub used: usize,
    pub spills: u64,
}

impl Budget {
    pub fn new(watermark: usize) -> Self {
        Budget {
            watermark: watermark.max(1),
            used: 0,
            spills: 0,
        }
    }

    /// Account `bytes` of scratch; returns true when a spill is due.
    pub fn account(&mut self, bytes: usize) -> bool {
        self.used += bytes;
        self.used > self.watermark
    }

    /// Mark a completed spill of `bytes`.
    pub fn spilled(&mut self, bytes: usize) {
        self.used = self.used.saturating_sub(bytes);
        self.spills += 1;
    }

    pub fn pressure(&self) -> f64 {
        self.used as f64 / self.watermark as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn watermark_triggers_spill() {
        let mut b = Budget::new(100);
        assert!(!b.account(60));
        assert!(b.account(50));
        b.spilled(80);
        assert_eq!(b.spills, 1);
        assert!(b.pressure() < 1.0);
    }

    #[test]
    fn pressure_ratio_tracks() {
        let mut b = Budget::new(200);
        b.account(100);
        assert!((b.pressure() - 0.5).abs() < 1e-9);
    }
}
