//! Storage flows: writer/reader round trips, store residency, merges.

use loam::format::{self, merge, FileWriter, ZoneMap};
use loam::page::{plain, write_page};
use loam::schema::tags;
use loam::store::Store;

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
fn store_tracks_resident_rows() {
    let ba = file(&[1, 2], 1, 2);
    let bb = file(&[5, 6, 7], 5, 7);
    let ia = format::open(&ba).unwrap();
    let ib = format::open(&bb).unwrap();
    let mut store = Store::default();
    store.add(ia);
    store.add(ib);
    assert_eq!(store.len(), 2);
    assert_eq!(store.total_rows(), 5);
}

#[test]
fn merged_file_scans() {
    let ba = file(&[1, 2], 1, 2);
    let bb = file(&[5, 6, 7], 5, 7);
    let ia = format::open(&ba).unwrap();
    let ib = format::open(&bb).unwrap();
    let bytes = merge::merge(&ia, &ib).unwrap();
    let mut engine = loam::Engine::new();
    let image = engine.open_bytes(&bytes).unwrap();
    let ranges = engine.plan_scan(&image, 4, 9).unwrap();
    assert_eq!(ranges.len(), 1);
    assert_eq!(ranges[0].group, 1);
}

#[test]
fn writer_offsets_resolve() {
    let bytes = file(&[10, 20, 30], 10, 30);
    let image = format::open(&bytes).unwrap();
    let page_ref = &image.groups[0].pages[0];
    let blob = image.page_bytes(page_ref).unwrap();
    let mut r = loam::bytes::Reader::new(blob);
    let header = loam::page::PageHeader::parse(&mut r).unwrap();
    assert_eq!(header.rows, 3);
}
