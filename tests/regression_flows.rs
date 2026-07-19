//! Regression flows: chains that combine migration, append, merge, and
//! validation in one lifecycle.

use loam::format::{self, append, merge, migrate, validate, FileWriter, ZoneMap};
use loam::page::{plain, write_page};
use loam::schema::tags;

fn base_file(version: u16, rows: &[i64], min: i64, max: i64) -> Vec<u8> {
    let mut w = FileWriter::new(version);
    w.column(1, "v", tags::TYPE_I64, tags::ENC_PLAIN, false);
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
fn migrate_append_merge_validate_chain() {
    // v1 base -> migrate to v2 -> append a group -> merge another file -> validate.
    let v1 = base_file(format::VERSION_V1, &[1, 2, 3], 1, 3);
    let v1_img = format::open(&v1).unwrap();
    let v2 = migrate::migrate_v1_to_v2(&v1_img).unwrap();
    let v2_img = format::open(&v2).unwrap();
    validate::validate(&v2_img).unwrap();

    let new_page = write_page(tags::ENC_PLAIN, 2, 0, &plain::write_i64(&[50, 60]));
    let appended = append::append_group(
        &v2_img,
        vec![(new_page, 2, tags::ENC_PLAIN)],
        ZoneMap {
            min: 50,
            max: 60,
            rows: 2,
        },
    )
    .unwrap();
    let appended_img = format::open(&appended).unwrap();
    assert_eq!(appended_img.groups.len(), 2);

    // Schema versions differ after migration (legacy chunk), so merge with
    // another migrated file instead.
    let other_base = base_file(format::VERSION_V1, &[90], 90, 90);
    let other_v1_img = format::open(&other_base).unwrap();
    let other_v2 = migrate::migrate_v1_to_v2(&other_v1_img).unwrap();
    let other_img = format::open(&other_v2).unwrap();
    let merged = merge::merge(&appended_img, &other_img).unwrap();
    let final_img = format::open(&merged).unwrap();
    assert_eq!(final_img.groups.len(), 3);
    validate::validate(&final_img).unwrap();
}

#[test]
fn inspect_after_chain() {
    let v1 = base_file(format::VERSION_V1, &[5, 6], 5, 6);
    let v2 = migrate::migrate_v1_to_v2(&format::open(&v1).unwrap()).unwrap();
    let img = format::open(&v2).unwrap();
    let text = format::inspect::render(&format::inspect::inspect(&img));
    assert!(text.contains("chunk_v1"));
    assert!(text.contains("fast:1"));
}

#[test]
fn query_after_chain() {
    let v1 = base_file(format::VERSION_V1, &[5, 6], 5, 6);
    let v2 = migrate::migrate_v1_to_v2(&format::open(&v1).unwrap()).unwrap();
    let img = format::open(&v2).unwrap();
    let new_page = write_page(tags::ENC_PLAIN, 1, 0, &plain::write_i64(&[70]));
    let appended = append::append_group(
        &img,
        vec![(new_page, 1, tags::ENC_PLAIN)],
        ZoneMap {
            min: 70,
            max: 70,
            rows: 1,
        },
    )
    .unwrap();
    let mut engine = loam::Engine::new();
    let final_img = engine.open_bytes(&appended).unwrap();
    let q = loam::query::parse("select c1 from t where v > 60").unwrap();
    assert_eq!(loam::query::run(&mut engine, &final_img, &q).unwrap(), 1);
}
