//! Append a row group to an existing file image, producing a new file.
//!
//! Appends are how incremental ingests land: the existing image keeps its
//! schema; the new group's zone map folds into the footer.

use crate::error::{Error, Result};
use crate::format::{FileImage, FileWriter, ZoneMap};

/// Append `pages` (as (page bytes, rows, encoding)) with their zone map to
/// an opened image, yielding a new file's bytes.
pub fn append_group(
    image: &FileImage<'_>,
    pages: Vec<(Vec<u8>, u32, u32)>,
    zone: ZoneMap,
) -> Result<Vec<u8>> {
    if pages.is_empty() {
        return Err(Error::LengthMismatch {
            what: "append group",
        });
    }
    let mut w = FileWriter::new(image.version);
    for c in image.schema.columns() {
        if image.version == crate::format::VERSION_V2
            && c.chunk_version == crate::format::VERSION_V1
        {
            w.column_legacy(c.id, &c.name, c.type_tag, c.encoding, c.dictionary);
        } else {
            w.column(c.id, &c.name, c.type_tag, c.encoding, c.dictionary);
        }
    }
    for (i, group) in image.groups.iter().enumerate() {
        let mut existing = Vec::new();
        for page_ref in &group.pages {
            existing.push((
                image.page_bytes(page_ref)?.to_vec(),
                page_ref.rows,
                page_ref.encoding,
            ));
        }
        w.row_group(group.footer_rows, existing, image.zones[i]);
    }
    let footer_rows: u32 = pages.iter().map(|(_, rows, _)| *rows).sum();
    w.row_group(footer_rows, pages, zone);
    Ok(w.finish())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::{self, FileWriter, ZoneMap};
    use crate::page::{plain, write_page};
    use crate::schema::tags;

    #[test]
    fn append_extends_file() {
        let mut w = FileWriter::new(format::VERSION_V2);
        w.column(1, "a", tags::TYPE_I64, tags::ENC_PLAIN, false);
        let p1 = write_page(tags::ENC_PLAIN, 2, 0, &plain::write_i64(&[1, 2]));
        w.row_group(
            2,
            vec![(p1, 2, tags::ENC_PLAIN)],
            ZoneMap {
                min: 1,
                max: 2,
                rows: 2,
            },
        );
        let bytes = w.finish();
        let image = format::open(&bytes).unwrap();
        let p2 = write_page(tags::ENC_PLAIN, 2, 0, &plain::write_i64(&[8, 9]));
        let appended = append_group(
            &image,
            vec![(p2, 2, tags::ENC_PLAIN)],
            ZoneMap {
                min: 8,
                max: 9,
                rows: 2,
            },
        )
        .unwrap();
        let extended = format::open(&appended).unwrap();
        assert_eq!(extended.groups.len(), 2);
        assert_eq!(extended.zones[1].min, 8);
        assert_eq!(extended.physical_rows(1), 2);
    }
}
