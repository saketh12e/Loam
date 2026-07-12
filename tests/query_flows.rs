//! Query, pruning, sidecar, and inspection flows over real files.

use loam::format::{self, stats_io, FileWriter, ZoneMap};
use loam::page::{plain, write_page};
use loam::query;
use loam::schema::tags;

fn wide_file() -> Vec<u8> {
    let mut w = FileWriter::new(format::VERSION_V2);
    w.column(1, "temp", tags::TYPE_I64, tags::ENC_PLAIN, false);
    w.column(2, "site", tags::TYPE_UTF8, tags::ENC_DICT, true);
    for g in 0..3 {
        let base = (g * 100) as i64;
        let page = write_page(
            tags::ENC_PLAIN,
            4,
            0,
            &plain::write_i64(&[base + 1, base + 2, base + 3, base + 4]),
        );
        w.row_group(
            4,
            vec![(page, 4, tags::ENC_PLAIN)],
            ZoneMap {
                min: base + 1,
                max: base + 4,
                rows: 4,
            },
        );
    }
    w.finish()
}

#[test]
fn query_scans_all_groups_without_filter() {
    let bytes = wide_file();
    let mut engine = loam::Engine::new();
    let image = engine.open_bytes(&bytes).unwrap();
    let q = query::parse("select c1 from t").unwrap();
    assert_eq!(query::run(&mut engine, &image, &q).unwrap(), 12);
}

#[test]
fn query_bounds_prune_to_tail() {
    let bytes = wide_file();
    let mut engine = loam::Engine::new();
    let image = engine.open_bytes(&bytes).unwrap();
    let q = query::parse("select c1 from t where temp >= 201 limit 100").unwrap();
    assert_eq!(query::run(&mut engine, &image, &q).unwrap(), 4);
}

#[test]
fn sidecar_round_trips_file_zones() {
    let bytes = wide_file();
    let image = format::open(&bytes).unwrap();
    let blob = stats_io::write_sidecar(&image.zones);
    let zones = stats_io::read_sidecar(&blob).unwrap();
    assert_eq!(zones.len(), 3);
    assert_eq!(zones[2].max, 204);
}

#[test]
fn sidecar_merge_unions_ranges() {
    let bytes = wide_file();
    let image = format::open(&bytes).unwrap();
    let merged = stats_io::merge_sidecars(&image.zones, &image.zones).unwrap();
    assert_eq!(merged[1].rows, 8);
}

#[test]
fn inspection_lists_all_groups() {
    let bytes = wide_file();
    let image = format::open(&bytes).unwrap();
    let text = format::inspect::render(&format::inspect::inspect(&image));
    assert!(text.contains("group 0"));
    assert!(text.contains("group 2"));
    assert!(text.contains("dict"));
}

#[test]
fn query_rejects_bad_column_and_text() {
    let bytes = wide_file();
    let mut engine = loam::Engine::new();
    let image = engine.open_bytes(&bytes).unwrap();
    assert!(
        query::parse("select c3 from t").is_err()
            || query::run(
                &mut engine,
                &image,
                &query::parse("select c3 from t").unwrap()
            )
            .is_err()
    );
    assert!(query::parse("delete from t").is_err());
}
