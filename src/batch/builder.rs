//! RecordBatch construction: column-wise assembly with row agreement and
//! validity tracking.

use crate::batch::decode::Column;
use crate::batch::validity::Validity;
use crate::error::{Error, Result};

#[derive(Default)]
pub struct BatchBuilder {
    columns: Vec<Column>,
    validity: Option<Validity>,
}

impl BatchBuilder {
    pub fn new() -> Self {
        BatchBuilder::default()
    }

    pub fn column(mut self, col: Column) -> Self {
        self.columns.push(col);
        self
    }

    pub fn with_validity(mut self, validity: Validity) -> Self {
        self.validity = Some(validity);
        self
    }

    pub fn rows(&self) -> usize {
        self.columns.first().map(|c| c.rows()).unwrap_or(0)
    }

    /// Freeze into a checked batch.
    pub fn finish(self) -> Result<BuiltBatch> {
        if self.columns.is_empty() {
            return Err(Error::LengthMismatch {
                what: "batch columns",
            });
        }
        let rows = self.rows();
        for col in &self.columns {
            if col.rows() != rows {
                return Err(Error::LengthMismatch {
                    what: "batch row agreement",
                });
            }
        }
        if let Some(v) = &self.validity {
            if v.rows() != rows {
                return Err(Error::LengthMismatch {
                    what: "batch validity rows",
                });
            }
        }
        Ok(BuiltBatch {
            columns: self.columns,
            validity: self.validity,
            rows,
        })
    }
}

pub struct BuiltBatch {
    pub columns: Vec<Column>,
    pub validity: Option<Validity>,
    pub rows: usize,
}

impl BuiltBatch {
    pub fn valid_rows(&self) -> usize {
        match &self.validity {
            Some(v) => v.count_valid(),
            None => self.rows,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_checked_batch() {
        let batch = BatchBuilder::new()
            .column(Column::I64(vec![1, 2]))
            .column(Column::Utf8(vec![b"a".to_vec(), b"b".to_vec()]))
            .finish()
            .unwrap();
        assert_eq!(batch.rows, 2);
        assert_eq!(batch.valid_rows(), 2);
    }

    #[test]
    fn rejects_row_disagreement() {
        assert!(BatchBuilder::new()
            .column(Column::I64(vec![1, 2]))
            .column(Column::I64(vec![1]))
            .finish()
            .is_err());
    }

    #[test]
    fn rejects_validity_mismatch() {
        assert!(BatchBuilder::new()
            .column(Column::I64(vec![1, 2]))
            .with_validity(Validity::new(3))
            .finish()
            .is_err());
    }
}
