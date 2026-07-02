//! Format stress: multi-group multi-column files, migration chains, merges,
//! and inspection over larger images.

use loam::format::{self, merge, migrate, FileWriter, ZoneMap};
use loam::page::{plain, write_page};
use loam::schema::tags;

fn big_file(version: u16, groups: usize, cols: usize) -> Vec<u8> {
    let mut w = FileWriter::new(version);
    for c in 0..cols {
        w.column(
            (c + 1) as u32,
            &format!("col{c}"),
            tags::TYPE_I64,
            tags::ENC_PLAIN,
            false,
        );
    }
    for g in 0..groups {
        let base = (g * 100) as i64;
        let page = write_page(tags::ENC_PLAIN, 8, 0, &plain::write_i64(&[base; 8]));
        w.row_group(
            8,
            vec![(page, 8, tags::ENC_PLAIN)],
            ZoneMap {
                min: base,
                max: base + 7,
                rows: 8,
            },
        );
    }
    w.finish()
}

#[test]
fn opens_wide_long_files() {
    let bytes = big_file(format::VERSION_V2, 24, 8);
    let image = format::open(&bytes).unwrap();
    assert_eq!(image.groups.len(), 24);
    assert_eq!(image.schema.columns().len(), 8);
    assert_eq!(image.physical_rows(23), 8);
}

#[test]
fn migration_chain_v1_to_v2() {
    let v1 = big_file(format::VERSION_V1, 4, 3);
    let image = format::open(&v1).unwrap();
    let v2 = migrate::migrate_v1_to_v2(&image).unwrap();
    let migrated = format::open(&v2).unwrap();
    assert_eq!(migrated.version, format::VERSION_V2);
    assert_eq!(migrated.groups.len(), 4);
    for col in migrated.schema.columns() {
        assert_eq!(col.chunk_version, format::VERSION_V1);
    }
}

#[test]
fn merge_chains_three_files() {
    let ba = big_file(format::VERSION_V2, 2, 2);
    let bb = big_file(format::VERSION_V2, 2, 2);
    let bc = big_file(format::VERSION_V2, 1, 2);
    let a = format::open(&ba).unwrap();
    let b = format::open(&bb).unwrap();
    let ab = merge::merge(&a, &b).unwrap();
    let ab_img = format::open(&ab).unwrap();
    let c = format::open(&bc).unwrap();
    let abc = merge::merge(&ab_img, &c).unwrap();
    let final_img = format::open(&abc).unwrap();
    assert_eq!(final_img.groups.len(), 5);
}

#[test]
fn inspection_scales() {
    let bytes = big_file(format::VERSION_V2, 16, 4);
    let image = format::open(&bytes).unwrap();
    let lines = format::inspect::inspect(&image);
    assert!(lines.len() > 16 + 4);
}

#[test]
fn scan_covers_all_ranges() {
    let bytes = big_file(format::VERSION_V2, 6, 2);
    let mut engine = loam::Engine::new();
    let image = engine.open_bytes(&bytes).unwrap();
    let ranges = engine.plan_scan(&image, i64::MIN, i64::MAX).unwrap();
    assert_eq!(ranges.len(), 6);
    let total: u32 = ranges.iter().map(|r| r.row_len).sum();
    assert_eq!(total, 48);
}
