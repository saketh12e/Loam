//! Validity bitmaps: per-row null tracking for columnar batches.

use crate::error::{Error, Result};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Validity {
    bits: Vec<u8>,
    rows: usize,
}

impl Validity {
    pub fn new(rows: usize) -> Self {
        Validity {
            bits: vec![0xff; rows.div_ceil(8)],
            rows,
        }
    }

    pub fn set(&mut self, row: usize, valid: bool) -> Result<()> {
        if row >= self.rows {
            return Err(Error::LengthMismatch {
                what: "validity row",
            });
        }
        let byte = row / 8;
        let mask = 1u8 << (row % 8);
        if valid {
            self.bits[byte] |= mask;
        } else {
            self.bits[byte] &= !mask;
        }
        Ok(())
    }

    pub fn is_valid(&self, row: usize) -> bool {
        if row >= self.rows {
            return false;
        }
        self.bits[row / 8] & (1u8 << (row % 8)) != 0
    }

    pub fn count_valid(&self) -> usize {
        (0..self.rows).filter(|&r| self.is_valid(r)).count()
    }

    pub fn rows(&self) -> usize {
        self.rows
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_and_count() {
        let mut v = Validity::new(10);
        assert_eq!(v.count_valid(), 10);
        v.set(3, false).unwrap();
        v.set(9, false).unwrap();
        assert!(!v.is_valid(3));
        assert!(!v.is_valid(9));
        assert_eq!(v.count_valid(), 8);
        assert!(v.set(10, false).is_err());
        assert!(!v.is_valid(11));
    }
}
