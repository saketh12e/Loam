//! Session metering: per-operation latency buckets for support diagnostics.
//! Buckets are logarithmic; summaries render inline in admin reports.

#[derive(Debug, Default, Clone, Copy)]
pub struct Meter {
    pub buckets: [u64; 8],
}

impl Meter {
    /// Record a duration in microseconds into its bucket.
    pub fn record(&mut self, micros: u64) {
        let idx = match micros {
            0..=9 => 0,
            10..=49 => 1,
            50..=199 => 2,
            200..=999 => 3,
            1000..=4999 => 4,
            5000..=19999 => 5,
            20000..=99999 => 6,
            _ => 7,
        };
        self.buckets[idx] += 1;
    }

    pub fn total(&self) -> u64 {
        self.buckets.iter().sum()
    }

    /// Render as a compact histogram line.
    pub fn render(&self) -> String {
        let mut out = String::new();
        let labels = ["<10u", "10u", "50u", "200u", "1ms", "5ms", "20ms", "100ms+"];
        for (label, count) in labels.iter().zip(self.buckets.iter()) {
            if *count > 0 {
                out.push_str(&format!("{label}:{count} "));
            }
        }
        out.trim_end().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buckets_cover_ranges() {
        let mut m = Meter::default();
        m.record(5);
        m.record(500);
        m.record(500_000);
        assert_eq!(m.total(), 3);
        let text = m.render();
        assert!(text.contains("<10u:1"));
        assert!(text.contains("200u:1"));
        assert!(text.contains("100ms+:1"));
    }
}
