//! Type-aware decode of pages into column batches.
//!
//! The scan path decodes a page into a typed `Column` given the column's
//! declared type and the page's encoding. Encoded variants route through
//! their kernels; plain pages reinterpret directly.

use crate::error::{Error, Result};
use crate::page::{delta, dict, plain, ContainerPage};
use crate::schema::tags;

/// One decoded column of values.
#[derive(Debug, Clone, PartialEq)]
pub enum Column {
    I64(Vec<i64>),
    F64(Vec<f64>),
    Utf8(Vec<Vec<u8>>),
}

impl Column {
    pub fn rows(&self) -> usize {
        match self {
            Column::I64(v) => v.len(),
            Column::F64(v) => v.len(),
            Column::Utf8(v) => v.len(),
        }
    }

    /// Values rendered as i64 where representable (utf8 is skipped).
    pub fn as_i64_lossy(&self) -> Vec<i64> {
        match self {
            Column::I64(v) => v.clone(),
            Column::F64(v) => v.iter().map(|f| *f as i64).collect(),
            Column::Utf8(_) => Vec::new(),
        }
    }
}

/// Decode one page given its column type.
pub fn decode_page(page: &ContainerPage, type_tag: u8) -> Result<Column> {
    let enc = page.header.encoding;
    match type_tag {
        tags::TYPE_I64 => match enc {
            t if t == tags::ENC_PLAIN => Ok(Column::I64(plain::parse_i64(
                &page.payload,
                page.header.rows,
            )?)),
            t if t == tags::ENC_DELTA => {
                Ok(Column::I64(delta::parse(&page.payload, page.header.rows)?))
            }
            other => Err(Error::UnsupportedTag { tag: other }),
        },
        tags::TYPE_F64 => match enc {
            t if t == tags::ENC_PLAIN => Ok(Column::F64(plain::parse_f64(
                &page.payload,
                page.header.rows,
            )?)),
            other => Err(Error::UnsupportedTag { tag: other }),
        },
        tags::TYPE_UTF8 => match enc {
            t if t == tags::ENC_PLAIN => Ok(Column::Utf8(plain::parse_utf8(
                &page.payload,
                page.header.rows,
            )?)),
            t if t == tags::ENC_DICT => {
                let d = dict::parse(&page.payload)?;
                Ok(Column::Utf8(d.words))
            }
            other => Err(Error::UnsupportedTag { tag: other }),
        },
        other => Err(Error::UnsupportedTag { tag: other as u32 }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::page::{write_page, PageHeader};

    fn container(enc: u32, rows: u32, payload: Vec<u8>) -> ContainerPage {
        let raw = write_page(enc, rows, 0, &payload);
        let (mut pages, _) =
            crate::page::parse_container(&crate::page::write_container(&[raw], 0)).unwrap();
        pages.remove(0)
    }

    #[test]
    fn decodes_plain_i64() {
        let page = container(tags::ENC_PLAIN, 2, plain::write_i64(&[7, 9]));
        assert_eq!(
            decode_page(&page, tags::TYPE_I64).unwrap(),
            Column::I64(vec![7, 9])
        );
    }

    #[test]
    fn decodes_delta_i64() {
        let page = container(tags::ENC_DELTA, 3, delta::write(&[10, 12, 11]));
        assert_eq!(
            decode_page(&page, tags::TYPE_I64).unwrap(),
            Column::I64(vec![10, 12, 11])
        );
    }

    #[test]
    fn decodes_plain_f64() {
        let page = container(tags::ENC_PLAIN, 2, plain::write_f64(&[1.5, 2.5]));
        assert_eq!(
            decode_page(&page, tags::TYPE_F64).unwrap(),
            Column::F64(vec![1.5, 2.5])
        );
    }

    #[test]
    fn decodes_plain_utf8() {
        let page = container(tags::ENC_PLAIN, 2, plain::write_utf8(&[b"a", b"b"]));
        assert_eq!(
            decode_page(&page, tags::TYPE_UTF8).unwrap(),
            Column::Utf8(vec![b"a".to_vec(), b"b".to_vec()])
        );
    }

    #[test]
    fn rejects_type_encoding_mismatch() {
        let page = container(tags::ENC_RLE, 1, plain::write_i64(&[1]));
        assert!(decode_page(&page, tags::TYPE_UTF8).is_err());
        assert!(decode_page(&page, tags::TYPE_F64).is_err());
        let _ = PageHeader {
            encoding: 0,
            rows: 0,
            flags: 0,
        };
    }

    #[test]
    fn as_i64_lossy_conversions() {
        assert_eq!(Column::I64(vec![1, 2]).as_i64_lossy(), vec![1, 2]);
        assert_eq!(Column::F64(vec![1.9, -2.1]).as_i64_lossy(), vec![1, -2]);
        assert!(Column::Utf8(vec![b"a".to_vec()]).as_i64_lossy().is_empty());
    }
}
