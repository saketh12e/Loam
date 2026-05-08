//! Image merge: combine two single-schema images into one file.
//!
//! Merge concatenates row groups (zone maps recomputed across the pair) and
//! requires identical schemas and versions — the common case of appending a
//! fresh export to an existing dataset.

use crate::error::{Error, Result};
use crate::format::{FileImage, FileWriter};

/// Merge two images into a new byte stream.
pub fn merge(a: &FileImage<'_>, b: &FileImage<'_>) -> Result<Vec<u8>> {
    if a.version != b.version {
        return Err(Error::BadMagic {
            what: "merge version mismatch",
        });
    }
    let ca = a.schema.columns();
    let cb = b.schema.columns();
    if ca.len() != cb.len() {
        return Err(Error::LengthMismatch {
            what: "merge schema mismatch",
        });
    }
    for (x, y) in ca.iter().zip(cb.iter()) {
        if x.id != y.id || x.type_tag != y.type_tag || x.encoding != y.encoding {
            return Err(Error::LengthMismatch {
                what: "merge column mismatch",
            });
        }
    }
    let mut w = FileWriter::new(a.version);
    for c in ca {
        if a.version == crate::format::VERSION_V1 {
            w.column(c.id, &c.name, c.type_tag, c.encoding, c.dictionary);
        } else if c.chunk_version == crate::format::VERSION_V1 {
            w.column_legacy(c.id, &c.name, c.type_tag, c.encoding, c.dictionary);
        } else {
            w.column(c.id, &c.name, c.type_tag, c.encoding, c.dictionary);
        }
    }
    for image in [a, b] {
        for (i, group) in image.groups.iter().enumerate() {
            let mut pages = Vec::new();
            for page_ref in &group.pages {
                pages.push((
                    image.page_bytes(page_ref)?.to_vec(),
                    page_ref.rows,
                    page_ref.encoding,
                ));
            }
            w.row_group(group.footer_rows, pages, image.zones[i]);
        }
    }
    Ok(w.finish())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::{self, FileWriter, ZoneMap};
    use crate::page::{plain, write_page};
    use crate::schema::tags;

    fn file(rows: &[i64], min: i64, max: i64) -> Vec<u8> {
        let mut w = FileWriter::new(format::VERSION_V2);
        w.column(1, "a", tags::TYPE_I64, tags::ENC_PLAIN, false);
        let page = write_page(
            tags::ENC_PLAIN,
            rows.len() as u32,
            0,
            &plain::write_i64(rows),
        );
        w.row_group(
            rows.len() as u32,
            vec![(page, rows.len() as u32, tags::ENC_PLAIN)],
            ZoneMap {
                min,
                max,
                rows: rows.len() as u32,
            },
        );
        w.finish()
    }

    #[test]
    fn merges_two_images() {
        let ba = file(&[1, 2], 1, 2);
        let bb = file(&[8, 9], 8, 9);
        let fa = format::open(&ba).unwrap();
        let fb = format::open(&bb).unwrap();
        let merged = merge(&fa, &fb).unwrap();
        let image = format::open(&merged).unwrap();
        assert_eq!(image.groups.len(), 2);
        assert_eq!(image.zones[0].min, 1);
        assert_eq!(image.zones[1].max, 9);
    }

    #[test]
    fn rejects_schema_mismatch() {
        let ba = file(&[1], 1, 1);
        let fa = format::open(&ba).unwrap();
        let mut w = FileWriter::new(format::VERSION_V2);
        w.column(2, "b", tags::TYPE_I64, tags::ENC_PLAIN, false);
        let page = write_page(tags::ENC_PLAIN, 1, 0, &plain::write_i64(&[3]));
        w.row_group(
            1,
            vec![(page, 1, tags::ENC_PLAIN)],
            ZoneMap {
                min: 3,
                max: 3,
                rows: 1,
            },
        );
        let bb = w.finish();
        let fb = format::open(&bb).unwrap();
        assert!(merge(&fa, &fb).is_err());
    }
}
