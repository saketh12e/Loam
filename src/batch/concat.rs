//! Batch concatenation: append batches row-wise with type checks.
//!
//! Concatenation is how partial decode results combine before aggregation.
//! All inputs must agree on column types and counts.

use crate::batch::decode::Column;
use crate::error::{Error, Result};

/// Concatenate same-typed columns pairwise.
pub fn concat_columns(batches: &[Vec<Column>]) -> Result<Vec<Column>> {
    let first = batches.first().ok_or(Error::LengthMismatch {
        what: "concat batches",
    })?;
    let width = first.len();
    for b in batches {
        if b.len() != width {
            return Err(Error::LengthMismatch {
                what: "concat width",
            });
        }
    }
    let mut out = Vec::with_capacity(width);
    for idx in 0..width {
        let mut merged: Option<Column> = None;
        for b in batches {
            merged = Some(match (merged, &b[idx]) {
                (None, c) => c.clone(),
                (Some(Column::I64(mut acc)), Column::I64(v)) => {
                    acc.extend_from_slice(v);
                    Column::I64(acc)
                }
                (Some(Column::F64(mut acc)), Column::F64(v)) => {
                    acc.extend_from_slice(v);
                    Column::F64(acc)
                }
                (Some(Column::Utf8(mut acc)), Column::Utf8(v)) => {
                    acc.extend(v.iter().cloned());
                    Column::Utf8(acc)
                }
                _ => {
                    return Err(Error::TypeMismatch {
                        what: "concat column types",
                    })
                }
            });
        }
        out.push(merged.expect("non-empty"));
    }
    Ok(out)
}

/// Total rows across batches.
pub fn total_rows(batches: &[Vec<Column>]) -> usize {
    batches
        .iter()
        .map(|b| b.first().map(|c| c.rows()).unwrap_or(0))
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn concatenates_i64_batches() {
        let a = vec![Column::I64(vec![1, 2])];
        let b = vec![Column::I64(vec![3])];
        let out = concat_columns(&[a, b]).unwrap();
        assert_eq!(out, vec![Column::I64(vec![1, 2, 3])]);
    }

    #[test]
    fn rejects_type_mismatch() {
        let a = vec![Column::I64(vec![1])];
        let b = vec![Column::F64(vec![1.0])];
        assert!(concat_columns(&[a, b]).is_err());
    }

    #[test]
    fn counts_total_rows() {
        let a = vec![Column::I64(vec![1, 2])];
        let b = vec![Column::I64(vec![3])];
        assert_eq!(total_rows(&[a, b]), 3);
    }
}
