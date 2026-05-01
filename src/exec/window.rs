//! Running window aggregates over ordered streams (moving averages, lags).

use crate::error::{Error, Result};

/// Moving sum over `width` rows; the first `width - 1` outputs are partial.
pub fn moving_sum(values: &[i64], width: u32) -> Result<Vec<i64>> {
    if width == 0 {
        return Err(Error::LengthMismatch {
            what: "window width",
        });
    }
    let w = width as usize;
    let mut out = Vec::with_capacity(values.len());
    let mut acc = 0i64;
    for (i, &v) in values.iter().enumerate() {
        acc += v;
        if i >= w {
            acc -= values[i - w];
        }
        out.push(acc);
    }
    Ok(out)
}

/// Row-over-row delta (lag 1); the first row yields zero.
pub fn lag_delta(values: &[i64]) -> Vec<i64> {
    let mut out = Vec::with_capacity(values.len());
    let mut prev = 0i64;
    for (i, &v) in values.iter().enumerate() {
        out.push(if i == 0 { 0 } else { v - prev });
        prev = v;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moving_sum_windows() {
        assert_eq!(
            moving_sum(&[1, 2, 3, 4, 5], 3).unwrap(),
            vec![1, 3, 6, 9, 12]
        );
        assert!(moving_sum(&[1], 0).is_err());
    }

    #[test]
    fn lag_deltas() {
        assert_eq!(lag_delta(&[10, 13, 11, 20]), vec![0, 3, -2, 9]);
        assert_eq!(lag_delta(&[]), Vec::<i64>::new());
    }
}

/// Moving average over `width` rows (truncated mean of each window).
pub fn moving_avg(values: &[i64], width: u32) -> Result<Vec<i64>> {
    let sums = moving_sum(values, width)?;
    let w = width as i64;
    Ok(sums
        .iter()
        .enumerate()
        .map(|(i, s)| s / (i as i64 + 1).min(w).max(1))
        .collect())
}

/// Lag by `n`: output[i] = values[i - n], leading slots filled with `fill`.
pub fn lag(values: &[i64], n: usize, fill: i64) -> Vec<i64> {
    let mut out = vec![fill; values.len()];
    if n < values.len() {
        let (head, tail) = out.split_at_mut(n);
        let _ = head;
        tail.copy_from_slice(&values[..values.len() - n]);
    }
    out
}

/// Dense rank over sorted input (1-based, ties share a rank).
pub fn dense_rank(values: &[i64]) -> Vec<u32> {
    let mut out = Vec::with_capacity(values.len());
    let mut rank = 0u32;
    let mut prev: Option<i64> = None;
    for &v in values {
        if Some(v) != prev {
            rank += 1;
            prev = Some(v);
        }
        out.push(rank);
    }
    out
}

#[cfg(test)]
mod window_tests {
    use super::*;

    #[test]
    fn moving_avg_behaves() {
        let out = moving_avg(&[2, 4, 6, 8], 2).unwrap();
        assert_eq!(out, vec![2, 3, 5, 7]);
    }

    #[test]
    fn lag_shifts() {
        assert_eq!(lag(&[1, 2, 3], 1, 0), vec![0, 1, 2]);
    }

    #[test]
    fn dense_rank_ties() {
        assert_eq!(dense_rank(&[1, 1, 2, 2, 5]), vec![1, 1, 2, 2, 3]);
    }
}
