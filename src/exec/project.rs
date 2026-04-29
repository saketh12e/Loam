//! Projection: column selection over decoded batches.
//!
//! Projection is a compile-time decision applied per batch: the pipeline
//! keeps only the requested columns, preserving row order. Projection pushdown
//! (skipping the decode of unreferenced columns) is planned but not yet done.

use crate::batch::decode::Column;
use crate::error::{Error, Result};

/// Project a wide row (columns in schema order) down to `indices`.
pub fn project_columns(columns: &[Column], indices: &[u32]) -> Result<Vec<Column>> {
    let mut out = Vec::with_capacity(indices.len());
    for &i in indices {
        let col = columns
            .get(i as usize)
            .ok_or(Error::UnknownColumn { id: i })?;
        out.push(col.clone());
    }
    Ok(out)
}

/// Row count agreement check across a projection input.
pub fn rows_agree(columns: &[Column]) -> Result<usize> {
    let mut rows = None;
    for col in columns {
        match rows {
            None => rows = Some(col.rows()),
            Some(r) if r == col.rows() => {}
            Some(_) => {
                return Err(Error::LengthMismatch {
                    what: "projection input rows",
                })
            }
        }
    }
    Ok(rows.unwrap_or(0))
}

/// Reorder a projection into schema order (operators emit schema order).
pub fn schema_order(columns: Vec<(u32, Column)>) -> Vec<(u32, Column)> {
    let mut cols = columns;
    cols.sort_by_key(|(i, _)| *i);
    cols
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cols() -> Vec<Column> {
        vec![
            Column::I64(vec![1, 2]),
            Column::F64(vec![1.5, 2.5]),
            Column::Utf8(vec![b"a".to_vec(), b"b".to_vec()]),
        ]
    }

    #[test]
    fn projects_requested_columns() {
        let out = project_columns(&cols(), &[2, 0]).unwrap();
        assert_eq!(out.len(), 2);
        assert_eq!(out[0], Column::Utf8(vec![b"a".to_vec(), b"b".to_vec()]));
        assert_eq!(out[1], Column::I64(vec![1, 2]));
    }

    #[test]
    fn rejects_out_of_range_index() {
        assert!(project_columns(&cols(), &[7]).is_err());
    }

    #[test]
    fn row_agreement_checked() {
        assert_eq!(rows_agree(&cols()).unwrap(), 2);
        let bad = vec![Column::I64(vec![1, 2]), Column::I64(vec![1])];
        assert!(rows_agree(&bad).is_err());
    }

    #[test]
    fn schema_ordering_sorts_by_index() {
        let ordered = schema_order(vec![(2, Column::I64(vec![1])), (0, Column::I64(vec![2]))]);
        assert_eq!(ordered[0].0, 0);
        assert_eq!(ordered[1].0, 2);
    }
}
