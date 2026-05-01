//! Top-k operator: keep the k largest values, order preserved on output.

use crate::error::{Error, Result};

/// The k largest values in descending order.
pub fn top_k(values: &[i64], k: usize) -> Result<Vec<i64>> {
    if k == 0 {
        return Ok(Vec::new());
    }
    if values.len() > 1 << 24 {
        return Err(Error::BudgetExceeded {
            what: "top_k input",
        });
    }
    let mut sorted = values.to_vec();
    sorted.sort_unstable_by(|a, b| b.cmp(a));
    sorted.truncate(k);
    Ok(sorted)
}

/// The k smallest values in ascending order.
pub fn bottom_k(values: &[i64], k: usize) -> Result<Vec<i64>> {
    if k == 0 {
        return Ok(Vec::new());
    }
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    sorted.truncate(k);
    Ok(sorted)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn top_and_bottom() {
        let values = vec![5, 1, 9, 3, 7];
        assert_eq!(top_k(&values, 2).unwrap(), vec![9, 7]);
        assert_eq!(bottom_k(&values, 2).unwrap(), vec![1, 3]);
        assert_eq!(top_k(&values, 0).unwrap(), Vec::<i64>::new());
    }
}
