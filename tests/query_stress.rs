//! Query stress: complex filters, multi-group pruning, and limits over
//! wide files.

use loam::format::{FileWriter, ZoneMap};
use loam::page::{plain, write_page};
use loam::query;
use loam::schema::tags;

fn rolling_file(groups: usize) -> Vec<u8> {
    let mut w = FileWriter::new(loam::format::VERSION_V2);
    w.column(1, "v", tags::TYPE_I64, tags::ENC_PLAIN, false);
    for g in 0..groups {
        let base = (g * 50) as i64;
        let page = write_page(
            tags::ENC_PLAIN,
            6,
            0,
            &plain::write_i64(&[base, base + 10, base + 20, base + 30, base + 40, base + 49]),
        );
        w.row_group(
            6,
            vec![(page, 6, tags::ENC_PLAIN)],
            ZoneMap {
                min: base,
                max: base + 49,
                rows: 6,
            },
        );
    }
    w.finish()
}

#[test]
fn or_filter_scans_everything() {
    let bytes = rolling_file(6);
    let mut engine = loam::Engine::new();
    let image = engine.open_bytes(&bytes).unwrap();
    let q = query::parse("select c1 from t where v == 0 or v == 250").unwrap();
    let _ = query::run(&mut engine, &image, &q).unwrap();
}

#[test]
fn and_bounds_prune_middle() {
    let bytes = rolling_file(10);
    let mut engine = loam::Engine::new();
    let image = engine.open_bytes(&bytes).unwrap();
    let q = query::parse("select c1 from t where v > 200 and v < 300").unwrap();
    // Groups [200,249] and [250,299] both overlap [201,299].
    assert_eq!(query::run(&mut engine, &image, &q).unwrap(), 12);
}

#[test]
fn limits_under_group_size() {
    let bytes = rolling_file(4);
    let mut engine = loam::Engine::new();
    let image = engine.open_bytes(&bytes).unwrap();
    let q = query::parse("select c1 from t limit 2").unwrap();
    assert_eq!(query::run(&mut engine, &image, &q).unwrap(), 2);
}

#[test]
fn empty_match_scans_zero() {
    let bytes = rolling_file(4);
    let mut engine = loam::Engine::new();
    let image = engine.open_bytes(&bytes).unwrap();
    let q = query::parse("select c1 from t where v > 10000").unwrap();
    assert_eq!(query::run(&mut engine, &image, &q).unwrap(), 0);
}

#[test]
fn multi_column_query() {
    let bytes = rolling_file(2);
    let mut engine = loam::Engine::new();
    let image = engine.open_bytes(&bytes).unwrap();
    let q = query::parse("select c1, c1 from t").unwrap();
    assert!(query::run(&mut engine, &image, &q).is_ok());
}

#[test]
fn case_query_like_flow() {
    let bytes = rolling_file(3);
    let mut engine = loam::Engine::new();
    let image = engine.open_bytes(&bytes).unwrap();
    let q = query::parse("select c1 from t where v >= 100 limit 4").unwrap();
    assert_eq!(query::run(&mut engine, &image, &q).unwrap(), 4);
}

#[test]
fn negative_range_matches_nothing() {
    let bytes = rolling_file(2);
    let mut engine = loam::Engine::new();
    let image = engine.open_bytes(&bytes).unwrap();
    let q = query::parse("select c1 from t where v < -1").unwrap();
    assert_eq!(query::run(&mut engine, &image, &q).unwrap(), 0);
}
