//! Top-k and append flows.

use loam::exec::topk;
use loam::format::{self, append, FileWriter, ZoneMap};
use loam::page::{plain, write_page};
use loam::schema::tags;

#[test]
fn topk_over_large_stream() {
    let values: Vec<i64> = (0..10_000).map(|i| (i * 7919) % 10_000).collect();
    let top = topk::top_k(&values, 3).unwrap();
    assert_eq!(top, vec![9999, 9998, 9997]);
    let bottom = topk::bottom_k(&values, 2).unwrap();
    assert_eq!(bottom, vec![0, 1]);
}

#[test]
fn append_then_scan_flow() {
    let mut w = FileWriter::new(format::VERSION_V2);
    w.column(1, "a", tags::TYPE_I64, tags::ENC_PLAIN, false);
    let p1 = write_page(tags::ENC_PLAIN, 3, 0, &plain::write_i64(&[1, 2, 3]));
    w.row_group(
        3,
        vec![(p1, 3, tags::ENC_PLAIN)],
        ZoneMap {
            min: 1,
            max: 3,
            rows: 3,
        },
    );
    let bytes = w.finish();
    let image = format::open(&bytes).unwrap();
    let p2 = write_page(tags::ENC_PLAIN, 2, 0, &plain::write_i64(&[20, 30]));
    let p3 = write_page(tags::ENC_PLAIN, 2, 0, &plain::write_i64(&[40, 50]));
    let appended = append::append_group(
        &image,
        vec![(p2, 2, tags::ENC_PLAIN), (p3, 2, tags::ENC_PLAIN)],
        ZoneMap {
            min: 20,
            max: 50,
            rows: 4,
        },
    )
    .unwrap();
    let mut engine = loam::Engine::new();
    let extended = engine.open_bytes(&appended).unwrap();
    let ranges = engine.plan_scan(&extended, 25, 100).unwrap();
    // Only the appended group matches; it contributes one range per page.
    assert_eq!(ranges.len(), 2);
    let total: u32 = ranges.iter().map(|r| r.row_len).sum();
    assert_eq!(total, 4);
}

#[test]
fn append_rejects_empty_group() {
    let mut w = FileWriter::new(format::VERSION_V2);
    w.column(1, "a", tags::TYPE_I64, tags::ENC_PLAIN, false);
    let p1 = write_page(tags::ENC_PLAIN, 1, 0, &plain::write_i64(&[1]));
    w.row_group(
        1,
        vec![(p1, 1, tags::ENC_PLAIN)],
        ZoneMap {
            min: 1,
            max: 1,
            rows: 1,
        },
    );
    let bytes = w.finish();
    let image = format::open(&bytes).unwrap();
    assert!(append::append_group(
        &image,
        vec![],
        ZoneMap {
            min: 0,
            max: 0,
            rows: 0
        }
    )
    .is_err());
}
