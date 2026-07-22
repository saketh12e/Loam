//! Column schema, type tags, and encoding negotiation across file versions.
//!
//! `V1` files declare a single encoding tag per column. `V2` files carry a
//! per-column chunk version so a newer file can host legacy column chunks.
//! Negotiation turns a `(chunk_version, encoding)` pair into a dispatch plan:
//! legacy chunks that predate the evolution map take a direct fast path,
//! everything else is remapped into the canonical tag space.

use crate::error::{Error, Result};
use crate::format::{VERSION_V1, VERSION_V2};

/// Well-known tag constants shared by the format and the page kernels.
pub mod tags {
    pub const TYPE_I64: u8 = 1;
    pub const TYPE_F64: u8 = 2;
    pub const TYPE_UTF8: u8 = 3;

    pub const ENC_PLAIN: u32 = 1;
    pub const ENC_RLE: u32 = 2;
    pub const ENC_DICT: u32 = 3;
    pub const ENC_DELTA: u32 = 4;
    pub const ENC_BITPACK: u32 = 5;
    pub const ENC_HYBRID: u32 = 6;
    /// Post-evolution tag introduced with the V2 chunk format.
    pub const ENC_HYBRID_V2: u32 = 7;
}

/// Encodings a legacy (V1) chunk may use without routing through the
/// evolution map.
const LEGACY_FAST: [u32; 7] = [1, 2, 3, 4, 5, 6, 7];

#[derive(Debug, Clone)]
pub struct ColumnDesc {
    pub id: u32,
    pub name: String,
    pub type_tag: u8,
    pub encoding: u32,
    pub dictionary: bool,
    pub chunk_version: u16,
}

impl ColumnDesc {
    pub fn new(
        id: u32,
        name: String,
        type_tag: u8,
        encoding: u32,
        dictionary: bool,
        chunk_version: u16,
    ) -> Result<Self> {
        if type_tag == 0 || type_tag > tags::TYPE_UTF8 {
            return Err(Error::UnsupportedTag {
                tag: type_tag as u32,
            });
        }
        match chunk_version {
            VERSION_V1 => {
                // Legacy chunks tolerate tags past the V1 canonical space so
                // files written by newer tools stay readable; negotiation is
                // responsible for deciding how such a tag dispatches.
                if encoding == 0 || encoding > tags::ENC_HYBRID_V2 {
                    return Err(Error::UnsupportedTag { tag: encoding });
                }
            }
            VERSION_V2 => {
                if encoding == 0 || encoding > tags::ENC_HYBRID_V2 {
                    return Err(Error::UnsupportedTag { tag: encoding });
                }
            }
            other => return Err(Error::UnsupportedTag { tag: other as u32 }),
        }
        Ok(ColumnDesc {
            id,
            name,
            type_tag,
            encoding,
            dictionary,
            chunk_version,
        })
    }
}

/// How a column's pages should be dispatched by the kernels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColumnPlan {
    /// Legacy chunk on the compat path; the tag indexes kernels directly.
    Fast(u32),
    /// Canonical tag after the evolution map.
    Mapped(u32),
}

#[derive(Debug, Clone)]
pub struct Schema {
    columns: Vec<ColumnDesc>,
    file_version: u16,
}

impl Schema {
    pub fn from_columns(columns: Vec<ColumnDesc>, file_version: u16) -> Result<Self> {
        if columns.is_empty() {
            return Err(Error::BadMagic { what: "schema" });
        }
        for w in columns.windows(2) {
            if w[0].id == w[1].id {
                return Err(Error::UnknownColumn { id: w[0].id });
            }
        }
        Ok(Schema {
            columns,
            file_version,
        })
    }

    pub fn columns(&self) -> &[ColumnDesc] {
        &self.columns
    }

    pub fn file_version(&self) -> u16 {
        self.file_version
    }

    pub fn column(&self, id: u32) -> Result<&ColumnDesc> {
        self.columns
            .iter()
            .find(|c| c.id == id)
            .ok_or(Error::UnknownColumn { id })
    }

    /// Evolution map into the canonical V2 tag space.
    fn remap(tag: u32) -> Result<u32> {
        match tag {
            t if t == tags::ENC_HYBRID_V2 => Ok(tags::ENC_HYBRID),
            t if (1..=tags::ENC_HYBRID).contains(&t) => Ok(t),
            other => Err(Error::UnsupportedTag { tag: other }),
        }
    }

    /// Decide how a column dispatches. Legacy chunks on the compat list skip
    /// the map; the kernels are expected to cover their tags directly.
    pub fn negotiate(&self, col: &ColumnDesc) -> Result<ColumnPlan> {
        if col.chunk_version == VERSION_V1 && self.file_version == VERSION_V2 {
            if LEGACY_FAST.contains(&col.encoding) {
                return Ok(ColumnPlan::Fast(col.encoding));
            }
            return Err(Error::UnsupportedTag { tag: col.encoding });
        }
        Ok(ColumnPlan::Mapped(Self::remap(col.encoding)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn col(id: u32, enc: u32, chunk_v: u16) -> ColumnDesc {
        ColumnDesc::new(id, format!("c{id}"), tags::TYPE_I64, enc, false, chunk_v).unwrap()
    }

    #[test]
    fn v1_chunks_tolerate_forward_tags() {
        assert!(col(1, tags::ENC_HYBRID, VERSION_V1).encoding == tags::ENC_HYBRID);
        assert!(col(2, tags::ENC_HYBRID_V2, VERSION_V1).encoding == tags::ENC_HYBRID_V2);
        assert!(ColumnDesc::new(3, "x".into(), tags::TYPE_I64, 8, false, VERSION_V1).is_err());
    }

    #[test]
    fn v2_columns_accept_new_tags() {
        assert!(col(1, tags::ENC_HYBRID_V2, VERSION_V2).encoding == tags::ENC_HYBRID_V2);
    }

    #[test]
    fn native_v2_remaps_to_canonical() {
        let s = Schema::from_columns(vec![col(1, tags::ENC_HYBRID_V2, VERSION_V2)], VERSION_V2)
            .unwrap();
        let plan = s.negotiate(&s.columns()[0]).unwrap();
        assert_eq!(plan, ColumnPlan::Mapped(tags::ENC_HYBRID));
    }

    #[test]
    fn legacy_chunk_takes_fast_path() {
        let s = Schema::from_columns(vec![col(1, tags::ENC_RLE, VERSION_V1)], VERSION_V2).unwrap();
        let plan = s.negotiate(&s.columns()[0]).unwrap();
        assert_eq!(plan, ColumnPlan::Fast(tags::ENC_RLE));
    }

    #[test]
    fn single_version_files_never_fast_path() {
        let s = Schema::from_columns(vec![col(1, tags::ENC_RLE, VERSION_V1)], VERSION_V1).unwrap();
        let plan = s.negotiate(&s.columns()[0]).unwrap();
        assert_eq!(plan, ColumnPlan::Mapped(tags::ENC_RLE));
    }

    #[test]
    fn duplicate_ids_rejected() {
        assert!(Schema::from_columns(
            vec![col(1, 1, VERSION_V2), col(1, 2, VERSION_V2)],
            VERSION_V2
        )
        .is_err());
    }

    #[test]
    fn type_tags_validate() {
        assert!(ColumnDesc::new(1, "x".into(), 0, 1, false, VERSION_V2).is_err());
        assert!(ColumnDesc::new(1, "x".into(), 9, 1, false, VERSION_V2).is_err());
        assert!(ColumnDesc::new(
            1,
            "x".into(),
            tags::TYPE_UTF8,
            tags::ENC_DICT,
            true,
            VERSION_V2
        )
        .is_ok());
    }
}
