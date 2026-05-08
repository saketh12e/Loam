//! File migration: rewrite V1 images as V2.
//!
//! Migration re-serializes an opened V1 image with V2 section layout and
//! per-column chunk versions, preserving pages and footer data byte for byte.

use crate::error::Result;
use crate::format::{FileImage, FileWriter, ZoneMap, VERSION_V1, VERSION_V2};

/// Migrate an opened V1 image to a V2 byte stream.
pub fn migrate_v1_to_v2(image: &FileImage<'_>) -> Result<Vec<u8>> {
    if image.version != VERSION_V1 {
        return Err(crate::error::Error::BadMagic {
            what: "migrate source version",
        });
    }
    let mut w = FileWriter::new(VERSION_V2);
    for col in image.schema.columns() {
        w.column_legacy(
            col.id,
            &col.name,
            col.type_tag,
            col.encoding,
            col.dictionary,
        );
    }
    for (i, group) in image.groups.iter().enumerate() {
        let mut pages = Vec::new();
        for page_ref in &group.pages {
            let bytes = image.page_bytes(page_ref)?.to_vec();
            pages.push((bytes, page_ref.rows, page_ref.encoding));
        }
        let z: ZoneMap = image.zones[i];
        w.row_group(group.footer_rows, pages, z);
    }
    Ok(w.finish())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::{self, ZoneMap};
    use crate::page::{plain, write_page};
    use crate::schema::tags;

    #[test]
    fn v1_image_migrates_to_v2() {
        let mut w = FileWriter::new(VERSION_V1);
        w.column(1, "a", tags::TYPE_I64, tags::ENC_PLAIN, false);
        let page = write_page(tags::ENC_PLAIN, 2, 0, &plain::write_i64(&[5, 6]));
        w.row_group(
            2,
            vec![(page, 2, tags::ENC_PLAIN)],
            ZoneMap {
                min: 5,
                max: 6,
                rows: 2,
            },
        );
        let v1 = w.finish();
        let image = format::open(&v1).unwrap();
        let v2 = migrate_v1_to_v2(&image).unwrap();
        let migrated = format::open(&v2).unwrap();
        assert_eq!(migrated.version, VERSION_V2);
        assert_eq!(migrated.schema.columns()[0].chunk_version, VERSION_V1);
        assert_eq!(migrated.groups[0].footer_rows, 2);
        assert_eq!(migrated.physical_rows(0), 2);
    }

    #[test]
    fn rejects_v2_source() {
        let mut w = FileWriter::new(VERSION_V2);
        w.column(1, "a", tags::TYPE_I64, tags::ENC_PLAIN, false);
        let page = write_page(tags::ENC_PLAIN, 1, 0, &plain::write_i64(&[1]));
        w.row_group(
            1,
            vec![(page, 1, tags::ENC_PLAIN)],
            ZoneMap {
                min: 1,
                max: 1,
                rows: 1,
            },
        );
        let v2 = w.finish();
        let image = format::open(&v2).unwrap();
        assert!(migrate_v1_to_v2(&image).is_err());
    }
}
