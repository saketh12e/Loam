//! Merge sorted column runs (compaction of sorted runs into one).

use crate::error::{Error, Result};

/// Merge k sorted runs into one sorted run, stable across runs.
pub fn merge_runs(runs: &[Vec<i64>]) -> Result<Vec<i64>> {
    if runs.is_empty() {
        return Err(Error::LengthMismatch { what: "merge runs" });
    }
    let mut out = runs[0].clone();
    for run in &runs[1..] {
        out = crate::exec::sort::merge_sorted(&out, run);
    }
    Ok(out)
}

/// Verify a run is sorted (debugging aid for compaction tests).
pub fn is_sorted(run: &[i64]) -> bool {
    run.windows(2).all(|w| w[0] <= w[1])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merges_three_runs() {
        let out = merge_runs(&[vec![1, 5, 9], vec![2, 6], vec![3, 4, 10]]).unwrap();
        assert_eq!(out, vec![1, 2, 3, 4, 5, 6, 9, 10]);
        assert!(is_sorted(&out));
    }

    #[test]
    fn rejects_no_runs() {
        assert!(merge_runs(&[]).is_err());
    }
}
