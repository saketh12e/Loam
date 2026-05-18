//! Count-min sketch for distinct-count estimation over high-cardinality
//! columns (used when exact dictionaries are too large to keep resident).

const WIDTH: usize = 256;
const ROWS: usize = 4;

#[derive(Debug, Clone)]
pub struct CountMin {
    table: [[u32; WIDTH]; ROWS],
}

impl Default for CountMin {
    fn default() -> Self {
        CountMin {
            table: [[0; WIDTH]; ROWS],
        }
    }
}

impl CountMin {
    fn hash(seed: u64, value: &[u8]) -> usize {
        let mut h = seed ^ 0xcbf2_9ce4_8422_2325;
        for &b in value {
            h ^= b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        (h as usize) % WIDTH
    }

    pub fn add(&mut self, value: &[u8]) {
        for (row, table_row) in self.table.iter_mut().enumerate() {
            let idx = Self::hash(row as u64 + 1, value);
            table_row[idx] = table_row[idx].saturating_add(1);
        }
    }

    pub fn estimate(&self, value: &[u8]) -> u32 {
        (0..ROWS)
            .map(|row| self.table[row][Self::hash(row as u64 + 1, value)])
            .min()
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_repeated_values() {
        let mut s = CountMin::default();
        for _ in 0..10 {
            s.add(b"alpha");
        }
        s.add(b"beta");
        assert!(s.estimate(b"alpha") >= 10);
        assert!(s.estimate(b"beta") >= 1);
        assert_eq!(s.estimate(b"never-seen"), 0);
    }
}
