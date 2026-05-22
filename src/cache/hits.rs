//! Cache hit-rate accounting, sampled per epoch window.

#[derive(Debug, Default, Clone, Copy)]
pub struct HitStats {
    pub hits: u64,
    pub misses: u64,
    pub evictions: u64,
}

impl HitStats {
    pub fn hit_rate(&self) -> f64 {
        let total = self.hits + self.misses;
        if total == 0 {
            return 0.0;
        }
        self.hits as f64 / total as f64
    }

    pub fn record(&mut self, hit: bool) {
        if hit {
            self.hits += 1;
        } else {
            self.misses += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rates_track_outcomes() {
        let mut s = HitStats::default();
        s.record(true);
        s.record(true);
        s.record(false);
        assert!((s.hit_rate() - 2.0 / 3.0).abs() < 1e-9);
        assert_eq!(HitStats::default().hit_rate(), 0.0);
    }

    #[test]
    fn perfect_hit_rate() {
        let mut s = HitStats::default();
        s.record(true);
        assert_eq!(s.hit_rate(), 1.0);
    }
}
