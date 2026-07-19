//! Whole-engine workflow flows: file -> session -> query -> inspect chains.

use loam::format::{self, validate, FileWriter, ZoneMap};
use loam::page::{plain, write_page};
use loam::query;
use loam::schema::tags;
use loam::session::ScriptWriter;

fn dataset() -> Vec<u8> {
    let mut w = FileWriter::new(format::VERSION_V2);
    w.column(1, "region_code", tags::TYPE_I64, tags::ENC_PLAIN, false);
    for g in 0..5 {
        let base = (g * 1000) as i64;
        let page = write_page(
            tags::ENC_PLAIN,
            6,
            0,
            &plain::write_i64(&[
                base,
                base + 100,
                base + 200,
                base + 300,
                base + 400,
                base + 500,
            ]),
        );
        w.row_group(
            6,
            vec![(page, 6, tags::ENC_PLAIN)],
            ZoneMap {
                min: base,
                max: base + 500,
                rows: 6,
            },
        );
    }
    w.finish()
}

#[test]
fn open_validate_scan_inspect_chain() {
    let bytes = dataset();
    let mut engine = loam::Engine::new();
    let image = engine.open_bytes(&bytes).unwrap();
    validate::validate(&image).unwrap();
    let ranges = engine.plan_scan(&image, 1500, 2600).unwrap();
    assert!(!ranges.is_empty());
    let text = format::inspect::render(&format::inspect::inspect(&image));
    assert!(text.contains("group 4"));
}

#[test]
fn query_after_session_warmup() {
    let bytes = dataset();
    let mut engine = loam::Engine::new();
    let image = engine.open_bytes(&bytes).unwrap();
    let script = ScriptWriter::new()
        .ingest(1, &bytes[..64])
        .hold(1)
        .read(0)
        .close()
        .finish();
    loam::session::run_session_driver(&script).unwrap();
    let q = query::parse("select c1 from t where region_code > 2500").unwrap();
    assert!(query::run(&mut engine, &image, &q).unwrap() > 0);
}

#[test]
fn catalog_then_query() {
    let bytes = dataset();
    let image = format::open(&bytes).unwrap();
    let mut cat = loam::store::catalog::Catalog::default();
    cat.register("regions", &image);
    assert_eq!(cat.rows_of("regions").unwrap(), 30);
    let mut engine = loam::Engine::new();
    let q = query::parse("select c1 from regions limit 3").unwrap();
    assert_eq!(query::run(&mut engine, &image, &q).unwrap(), 3);
}

#[test]
fn wal_journal_of_workflow_ops() {
    let mut journal = Vec::new();
    loam::session::wal::append(&mut journal, 1, b"open:dataset");
    loam::session::wal::append(&mut journal, 2, b"scan:[1500,2600]");
    loam::session::wal::append(&mut journal, 3, b"snapshot:t1");
    let frames = loam::session::wal::recover(&journal).unwrap();
    assert_eq!(frames.len(), 3);
    assert_eq!(frames[2].payload, b"snapshot:t1");
}

#[test]
fn admin_review_of_chain() {
    let bytes = dataset();
    let mut engine = loam::Engine::new();
    let image = engine.open_bytes(&bytes).unwrap();
    validate::validate(&image).unwrap();
    let report = loam::engine_admin::report(&engine.metrics, &engine.diagnostics);
    assert!(report.text.contains("files=1"));
    assert!(report.events >= 1);
}
