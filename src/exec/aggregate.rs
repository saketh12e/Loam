//! Aggregate operators over value streams.
//!
//! Aggregates consume a (possibly grouped) i64 stream and produce summary
//! values. All operators are single-pass and numerically careful within the
//! engine's i64 domain (sum is checked; mean truncates).

use crate::error::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Aggregate {
    pub count: u64,
    pub sum: i64,
    pub min: i64,
    pub max: i64,
}

impl Aggregate {
    pub fn mean(&self) -> Option<i64> {
        if self.count == 0 {
            None
        } else {
            Some(self.sum / self.count as i64)
        }
    }
}

/// Fold a stream into an `Aggregate`.
pub fn fold(values: &[i64]) -> Result<Aggregate> {
    if values.is_empty() {
        return Err(Error::LengthMismatch {
            what: "aggregate input",
        });
    }
    let mut acc = Aggregate {
        count: 0,
        sum: 0,
        min: i64::MAX,
        max: i64::MIN,
    };
    for &v in values {
        acc.count += 1;
        acc.sum = acc.sum.checked_add(v).ok_or(Error::LengthMismatch {
            what: "aggregate sum",
        })?;
        acc.min = acc.min.min(v);
        acc.max = acc.max.max(v);
    }
    Ok(acc)
}

/// Population variance over a stream, truncated to i64.
pub fn variance(values: &[i64]) -> Result<i64> {
    if values.is_empty() {
        return Err(Error::LengthMismatch {
            what: "variance input",
        });
    }
    let mean = values.iter().sum::<i64>() / values.len() as i64;
    let mut acc: i128 = 0;
    for &v in values {
        let d = (v - mean) as i128;
        acc += d * d;
    }
    Ok((acc / values.len() as i128) as i64)
}

/// Exact median by sorted copy (odd count) or lower middle (even count).
pub fn median(values: &[i64]) -> Result<i64> {
    if values.is_empty() {
        return Err(Error::LengthMismatch {
            what: "median input",
        });
    }
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    Ok(sorted[(sorted.len() - 1) / 2])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fold_tracks_all_fields() {
        let acc = fold(&[4, -2, 9, 1]).unwrap();
        assert_eq!(acc.count, 4);
        assert_eq!(acc.sum, 12);
        assert_eq!(acc.min, -2);
        assert_eq!(acc.max, 9);
        assert_eq!(acc.mean(), Some(3));
    }

    #[test]
    fn variance_of_spread() {
        assert_eq!(variance(&[2, 4, 6]).unwrap(), 2);
    }

    #[test]
    fn median_odd_and_even() {
        assert_eq!(median(&[5, 1, 9]).unwrap(), 5);
        assert_eq!(median(&[8, 2, 10, 4]).unwrap(), 4);
    }

    #[test]
    fn rejects_empty_inputs() {
        assert!(fold(&[]).is_err());
        assert!(variance(&[]).is_err());
        assert!(median(&[]).is_err());
    }

    #[test]
    fn mean_handles_negatives() {
        let acc = fold(&[-10, 10]).unwrap();
        assert_eq!(acc.mean(), Some(0));
    }

    #[test]
    fn variance_zero_for_constant() {
        assert_eq!(variance(&[7, 7, 7]).unwrap(), 0);
    }
}
