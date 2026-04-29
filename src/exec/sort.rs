//! Order-by operator: bounded sorts over i64 streams with stable output.

use crate::error::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Asc,
    Desc,
}

/// Sort a stream, optionally capping the output (top-k).
pub fn order_by(values: &[i64], dir: Direction, cap: Option<usize>) -> Result<Vec<i64>> {
    if values.len() > 1 << 24 {
        return Err(Error::BudgetExceeded {
            what: "order_by input",
        });
    }
    let mut out = values.to_vec();
    match dir {
        Direction::Asc => out.sort_unstable(),
        Direction::Desc => out.sort_unstable_by(|a, b| b.cmp(a)),
    }
    if let Some(cap) = cap {
        out.truncate(cap);
    }
    Ok(out)
}

/// Merge two sorted runs into one sorted run (stable).
pub fn merge_sorted(a: &[i64], b: &[i64]) -> Vec<i64> {
    let mut out = Vec::with_capacity(a.len() + b.len());
    let (mut i, mut j) = (0, 0);
    while i < a.len() && j < b.len() {
        if a[i] <= b[j] {
            out.push(a[i]);
            i += 1;
        } else {
            out.push(b[j]);
            j += 1;
        }
    }
    out.extend_from_slice(&a[i..]);
    out.extend_from_slice(&b[j..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asc_desc_and_cap() {
        assert_eq!(
            order_by(&[3, 1, 2], Direction::Asc, None).unwrap(),
            vec![1, 2, 3]
        );
        assert_eq!(
            order_by(&[3, 1, 2], Direction::Desc, Some(2)).unwrap(),
            vec![3, 2]
        );
    }
}
