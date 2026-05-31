//! Named snapshots of the store: point-in-time table sets that embedded
//! callers can roll back to after experimental ingests.

use std::collections::HashMap;

use crate::error::{Error, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotMeta {
    pub name: String,
    pub tables: usize,
    pub rows: u64,
    pub epoch: u64,
}

pub struct Snapshots {
    metas: HashMap<String, SnapshotMeta>,
    epoch: u64,
}

impl Default for Snapshots {
    fn default() -> Self {
        Snapshots {
            metas: HashMap::new(),
            epoch: 1,
        }
    }
}

impl Snapshots {
    /// Capture a snapshot of the current table set.
    pub fn take(&mut self, name: &str, tables: usize, rows: u64) -> Result<()> {
        if self.metas.contains_key(name) {
            return Err(Error::View {
                detail: format!("snapshot '{name}' exists"),
            });
        }
        self.epoch += 1;
        self.metas.insert(
            name.to_string(),
            SnapshotMeta {
                name: name.to_string(),
                tables,
                rows,
                epoch: self.epoch,
            },
        );
        Ok(())
    }

    pub fn get(&self, name: &str) -> Result<&SnapshotMeta> {
        self.metas.get(name).ok_or(Error::View {
            detail: format!("no snapshot '{name}'"),
        })
    }

    pub fn drop(&mut self, name: &str) -> Result<SnapshotMeta> {
        self.metas.remove(name).ok_or(Error::View {
            detail: format!("no snapshot '{name}'"),
        })
    }

    pub fn list(&self) -> Vec<&SnapshotMeta> {
        let mut out: Vec<_> = self.metas.values().collect();
        out.sort_by_key(|m| m.epoch);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn take_list_drop() {
        let mut s = Snapshots::default();
        s.take("before_ingest", 2, 100).unwrap();
        s.take("after_ingest", 3, 250).unwrap();
        assert_eq!(s.list().len(), 2);
        assert_eq!(s.get("after_ingest").unwrap().rows, 250);
        assert!(s.take("before_ingest", 2, 100).is_err());
        assert!(s.drop("before_ingest").is_ok());
        assert!(s.drop("before_ingest").is_err());
    }
}
