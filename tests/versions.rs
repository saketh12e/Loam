//! Mixed-version files, migration, and inspection flows.

use loam::format::{self, migrate, FileWriter, ZoneMap};
use loam::page::{plain, write_page};
use loam::schema::{tags, ColumnPlan};

#[test]
fn v2_file_with_legacy_chunk_negotiates() {
    let mut w = FileWriter::new(format::VERSION_V2);
    w.column_legacy(1, "old", tags::TYPE_I64, tags::ENC_RLE, false);
    let page = write_page(tags::ENC_PLAIN, 2, 0, &plain::write_i64(&[1, 2]));
    w.row_group(
        2,
        vec![(page, 2, tags::ENC_PLAIN)],
        ZoneMap {
            min: 1,
            max: 2,
            rows: 2,
        },
    );
    let bytes = w.finish();
    let image = format::open(&bytes).unwrap();
    let col = &image.schema.columns()[0];
    assert_eq!(col.chunk_version, format::VERSION_V1);
    assert_eq!(
        image.schema.negotiate(col).unwrap(),
        ColumnPlan::Fast(tags::ENC_RLE)
    );
}

#[test]
fn v1_to_v2_migration_preserves_pages() {
    let mut w = FileWriter::new(format::VERSION_V1);
    w.column(1, "a", tags::TYPE_I64, tags::ENC_PLAIN, false);
    let page = write_page(tags::ENC_PLAIN, 3, 0, &plain::write_i64(&[7, 8, 9]));
    w.row_group(
        3,
        vec![(page, 3, tags::ENC_PLAIN)],
        ZoneMap {
            min: 7,
            max: 9,
            rows: 3,
        },
    );
    let v1 = w.finish();
    let image = format::open(&v1).unwrap();
    let v2 = migrate::migrate_v1_to_v2(&image).unwrap();
    let migrated = format::open(&v2).unwrap();
    assert_eq!(migrated.version, format::VERSION_V2);
    assert_eq!(migrated.physical_rows(0), 3);
    let bytes = migrated.page_bytes(&migrated.groups[0].pages[0]).unwrap();
    // Page blob: header, payload length prefix, then the payload itself.
    let mut r = loam::bytes::Reader::new(bytes);
    let header = loam::page::PageHeader::parse(&mut r).unwrap();
    let payload_len = r.varint("page payload length").unwrap() as usize;
    let payload = r.take(payload_len, "page payload").unwrap();
    let values = plain::parse_i64(payload, header.rows).unwrap();
    assert_eq!(values, vec![7, 8, 9]);
}

#[test]
fn inspection_renders_mixed_file() {
    let mut w = FileWriter::new(format::VERSION_V2);
    w.column(1, "a", tags::TYPE_I64, tags::ENC_PLAIN, false);
    w.column_legacy(2, "b", tags::TYPE_I64, tags::ENC_DELTA, false);
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
    let bytes = w.finish();
    let image = format::open(&bytes).unwrap();
    let text = format::inspect::render(&format::inspect::inspect(&image));
    assert!(text.contains("column c1 'a'"));
    assert!(text.contains("column c2 'b'"));
    assert!(text.contains("chunk_v1"));
}
