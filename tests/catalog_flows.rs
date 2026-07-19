//! Catalog, snapshot, and session WAL flows.

use loam::format::{FileWriter, ZoneMap};
use loam::page::{plain, write_page};
use loam::schema::tags;
use loam::session::wal;
use loam::store::{catalog::Catalog, snapshot::Snapshots};

fn image() -> Vec<u8> {
    let mut w = FileWriter::new(loam::format::VERSION_V2);
    w.column(1, "a", tags::TYPE_I64, tags::ENC_PLAIN, false);
    let page = write_page(tags::ENC_PLAIN, 3, 0, &plain::write_i64(&[1, 2, 3]));
    w.row_group(
        3,
        vec![(page, 3, tags::ENC_PLAIN)],
        ZoneMap {
            min: 1,
            max: 3,
            rows: 3,
        },
    );
    w.finish()
}

#[test]
fn catalog_registers_and_compares() {
    let bytes = image();
    let img = loam::format::open(&bytes).unwrap();
    let mut cat = Catalog::default();
    cat.register("metrics", &img);
    cat.register("metrics_copy", &img);
    assert!(cat.compatible("metrics", "metrics_copy").unwrap());
    assert_eq!(cat.rows_of("metrics").unwrap(), 3);
}

#[test]
fn snapshots_capture_and_drop() {
    let mut snaps = Snapshots::default();
    snaps.take("t0", 1, 3).unwrap();
    snaps.take("t1", 2, 6).unwrap();
    assert_eq!(snaps.list().len(), 2);
    assert!(snaps.drop("t0").is_ok());
    assert_eq!(snaps.list().len(), 1);
}

#[test]
fn wal_recovers_committed_prefix() {
    let mut journal = Vec::new();
    wal::append(&mut journal, 1, b"ingest:p1");
    wal::append(&mut journal, 2, b"ingest:p2");
    wal::append(&mut journal, 3, b"flush");
    let frames = wal::recover(&journal).unwrap();
    assert_eq!(frames.len(), 3);
    assert_eq!(frames[2].payload, b"flush");
}
