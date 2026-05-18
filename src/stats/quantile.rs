//! Quantile sketch: mergeable summaries for approximate percentiles over
//! large i64 columns (t-digest style, fixed-centroid).

use crate::error::{Error, Result};

const MAX_CENTROIDS: usize = 64;

#[derive(Debug, Clone, Default)]
pub struct Quantile {
    centroids: Vec<(i64, u64)>, // (mean, weight), mean-sorted
    count: u64,
}

impl Quantile {
    pub fn add(&mut self, value: i64) {
        self.count += 1;
        self.centroids.push((value, 1));
        if self.centroids.len() > MAX_CENTROIDS {
            self.compress();
        }
    }

    pub fn add_all(&mut self, values: &[i64]) {
        for &v in values {
            self.add(v);
        }
    }

    /// Merge the closest pair of centroids until within budget.
    fn compress(&mut self) {
        self.centroids.sort_by_key(|(m, _)| *m);
        while self.centroids.len() > MAX_CENTROIDS / 2 {
            let mut best = 1usize;
            let mut best_gap = i64::MAX;
            for i in 1..self.centroids.len() {
                let gap = self.centroids[i].0 - self.centroids[i - 1].0;
                if gap < best_gap {
                    best_gap = gap;
                    best = i;
                }
            }
            let (m1, w1) = self.centroids[best - 1];
            let (m2, w2) = self.centroids[best];
            let merged =
                ((m1 as i128 * w1 as i128 + m2 as i128 * w2 as i128) / (w1 + w2) as i128) as i64;
            self.centroids[best - 1] = (merged, w1 + w2);
            self.centroids.remove(best);
        }
    }

    /// Approximate value at quantile `q` in [0, 1].
    pub fn at(&self, q: f64) -> Result<i64> {
        if self.centroids.is_empty() {
            return Err(Error::LengthMismatch {
                what: "quantile sketch",
            });
        }
        let target = (self.count as f64 * q.clamp(0.0, 1.0)) as u64;
        let mut seen = 0u64;
        for &(mean, weight) in &self.centroids {
            seen += weight;
            if seen > target {
                return Ok(mean);
            }
        }
        Ok(self.centroids.last().expect("non-empty").0)
    }

    pub fn count(&self) -> u64 {
        self.count
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn median_of_range() {
        let mut q = Quantile::default();
        q.add_all(&(0..1000).collect::<Vec<_>>());
        let med = q.at(0.5).unwrap();
        assert!((400..=600).contains(&med), "med {med}");
    }

    #[test]
    fn tails_track_extremes() {
        let mut q = Quantile::default();
        q.add_all(&(0..100).collect::<Vec<_>>());
        assert!(q.at(0.0).unwrap() <= 2);
        assert!(q.at(1.0).unwrap() >= 97);
    }

    #[test]
    fn rejects_empty() {
        assert!(Quantile::default().at(0.5).is_err());
    }
}
