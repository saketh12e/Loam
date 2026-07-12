//! Query front-end and pruning coverage.

use loam::format::{FileWriter, ZoneMap};
use loam::page::{plain, write_page};
use loam::query;
use loam::schema::tags;
use loam::stats::histogram::Histogram;

fn two_group_file() -> Vec<u8> {
    let mut w = FileWriter::new(loam::format::VERSION_V2);
    w.column(1, "temp", tags::TYPE_I64, tags::ENC_PLAIN, false);
    let p1 = write_page(tags::ENC_PLAIN, 4, 0, &plain::write_i64(&[1, 2, 3, 4]));
    let p2 = write_page(tags::ENC_PLAIN, 4, 0, &plain::write_i64(&[50, 60, 70, 80]));
    w.row_group(
        4,
        vec![(p1, 4, tags::ENC_PLAIN)],
        ZoneMap {
            min: 1,
            max: 4,
            rows: 4,
        },
    );
    w.row_group(
        4,
        vec![(p2, 4, tags::ENC_PLAIN)],
        ZoneMap {
            min: 50,
            max: 80,
            rows: 4,
        },
    );
    w.finish()
}

#[test]
fn query_prunes_to_one_group() {
    let bytes = two_group_file();
    let mut engine = loam::Engine::new();
    let image = engine.open_bytes(&bytes).unwrap();
    let q = query::parse("select c1 from t where temp > 60").unwrap();
    assert_eq!(query::run(&mut engine, &image, &q).unwrap(), 4);
}

#[test]
fn query_limit_caps_rows() {
    let bytes = two_group_file();
    let mut engine = loam::Engine::new();
    let image = engine.open_bytes(&bytes).unwrap();
    let q = query::parse("select c1 from t limit 3").unwrap();
    assert_eq!(query::run(&mut engine, &image, &q).unwrap(), 3);
}

#[test]
fn unknown_column_rejected() {
    let bytes = two_group_file();
    let mut engine = loam::Engine::new();
    let image = engine.open_bytes(&bytes).unwrap();
    let q = query::parse("select c9 from t").unwrap();
    assert!(query::run(&mut engine, &image, &q).is_err());
}

#[test]
fn histogram_guides_skewed_ranges() {
    let values: Vec<i64> = (0..1000)
        .map(|i| if i % 10 == 0 { 5 } else { 500 })
        .collect();
    let h = Histogram::build(&values, 8).unwrap();
    let spikes = h.estimate(0, 10);
    let bulk = h.estimate(400, 600);
    assert!(bulk > spikes * 5, "spikes {spikes} bulk {bulk}");
}
