//! End-to-end storage flows: write a file, open it, scan it, decode pages.

use loam::format::{FileWriter, ZoneMap};
use loam::page::rle::Run;
use loam::page::{rle, write_container, write_page};
use loam::schema::tags;

fn plain_page(values: &[i64]) -> Vec<u8> {
    let mut payload = Vec::new();
    for v in values {
        payload.extend_from_slice(&v.to_le_bytes());
    }
    write_page(tags::ENC_PLAIN, values.len() as u32, 0, &payload)
}

#[test]
fn write_open_scan_flow() {
    let mut w = FileWriter::new(loam::format::VERSION_V2);
    w.column(1, "temp", tags::TYPE_I64, tags::ENC_PLAIN, false);
    w.row_group(
        4,
        vec![(plain_page(&[10, 20, 30, 40]), 4, tags::ENC_PLAIN)],
        ZoneMap {
            min: 10,
            max: 40,
            rows: 4,
        },
    );
    let bytes = w.finish();
    let mut engine = loam::Engine::new();
    let image = engine.open_bytes(&bytes).expect("open");
    let ranges = engine.plan_scan(&image, 15, 35).expect("scan plan");
    assert_eq!(ranges.len(), 1);
    assert_eq!(image.schema.columns()[0].name, "temp");
}

#[test]
fn page_container_decode_flow() {
    let runs = vec![Run { value: 3, len: 5 }, Run { value: 8, len: 2 }];
    let page = write_page(tags::ENC_RLE, 7, 0, &rle::write_runs(&runs));
    let container = write_container(&[page], 0);
    let decoded = loam::page::run_page_driver(&container).expect("decode");
    assert_eq!(decoded, 7);
}

#[test]
fn session_flow() {
    let script = loam::session::ScriptWriter::new()
        .ingest(1, &[1u8; 24])
        .hold(1)
        .read(0)
        .install_dict(1, &[b"x", b"y"])
        .query_dict(1, 3, &[0, 1])
        .close()
        .finish();
    loam::session::run_session_driver(&script).expect("session");
}
