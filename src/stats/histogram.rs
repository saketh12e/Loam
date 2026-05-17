//! Column histograms for selectivity estimation.
//!
//! Histograms are equal-width over i64 ranges and used by the optimizer when
//! zone maps are too coarse (skewed distributions inside one zone).

use crate::error::{Error, Result};

#[derive(Debug, Clone, PartialEq)]
pub struct Histogram {
    pub min: i64,
    pub width: u64,
    pub buckets: Vec<u64>,
}

impl Histogram {
    /// Build an equal-width histogram with `bucket_count` buckets.
    pub fn build(values: &[i64], bucket_count: usize) -> Result<Self> {
        if values.is_empty() || bucket_count == 0 {
            return Err(Error::LengthMismatch {
                what: "histogram input",
            });
        }
        let min = *values.iter().min().unwrap();
        let max = *values.iter().max().unwrap();
        let span = (max - min) as u64 + 1;
        let width = span.div_ceil(bucket_count as u64).max(1);
        let mut buckets = vec![0u64; bucket_count];
        for &v in values {
            let idx = ((v - min) as u64 / width).min(bucket_count as u64 - 1);
            buckets[idx as usize] += 1;
        }
        Ok(Histogram {
            min,
            width,
            buckets,
        })
    }

    /// Estimated rows in `[lo, hi]` (uniform within buckets).
    pub fn estimate(&self, lo: i64, hi: i64) -> u64 {
        let mut total = 0u64;
        for (i, &count) in self.buckets.iter().enumerate() {
            let b_lo = self.min + i as i64 * self.width as i64;
            let b_hi = b_lo + self.width as i64 - 1;
            if b_hi < lo || b_lo > hi {
                continue;
            }
            let overlap = (b_hi.min(hi) - b_lo.max(lo) + 1).max(0) as u64;
            total += count * overlap / self.width.max(1);
        }
        total
    }

    pub fn total(&self) -> u64 {
        self.buckets.iter().sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_and_estimates() {
        let values: Vec<i64> = (0..100).collect();
        let h = Histogram::build(&values, 10).unwrap();
        assert_eq!(h.total(), 100);
        let est = h.estimate(0, 9);
        assert!((9..=11).contains(&est), "est {est}");
    }

    #[test]
    fn single_value_histogram() {
        let h = Histogram::build(&[42], 4).unwrap();
        assert_eq!(h.total(), 1);
        assert_eq!(h.estimate(42, 42), 1);
    }

    #[test]
    fn rejects_empty() {
        assert!(Histogram::build(&[], 4).is_err());
    }
}
