//! Distinct operator: deduplicate an i64 stream preserving first-seen order.

use std::collections::HashSet;

use crate::error::{Error, Result};

/// Deduplicate preserving first-seen order.
pub fn distinct(values: &[i64]) -> Result<Vec<i64>> {
    if values.len() > 1 << 24 {
        return Err(Error::BudgetExceeded {
            what: "distinct input",
        });
    }
    let mut seen = HashSet::with_capacity(values.len());
    let mut out = Vec::new();
    for &v in values {
        if seen.insert(v) {
            out.push(v);
        }
    }
    Ok(out)
}

/// Distinct count without materializing the set contents.
pub fn distinct_count(values: &[i64]) -> usize {
    let mut seen = HashSet::with_capacity(values.len());
    values.iter().filter(|v| seen.insert(**v)).count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dedupes_in_order() {
        assert_eq!(distinct(&[3, 1, 3, 2, 1]).unwrap(), vec![3, 1, 2]);
        assert_eq!(distinct_count(&[3, 1, 3, 2, 1]), 3);
    }
}
