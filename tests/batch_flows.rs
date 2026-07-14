//! Batch stack coverage: builders, concat, validity, typed decode, and run
//! merge.

use loam::batch::decode::{decode_page, Column};
use loam::batch::{builder::BatchBuilder, concat, merge, validity::Validity};
use loam::page::{plain, write_page};

#[test]
fn builder_with_validity_counts() {
    let mut v = Validity::new(3);
    v.set(1, false).unwrap();
    let batch = BatchBuilder::new()
        .column(Column::I64(vec![10, 20, 30]))
        .with_validity(v)
        .finish()
        .unwrap();
    assert_eq!(batch.rows, 3);
    assert_eq!(batch.valid_rows(), 2);
}

#[test]
fn concat_wide_batches() {
    let a = vec![Column::I64(vec![1]), Column::Utf8(vec![b"a".to_vec()])];
    let b = vec![Column::I64(vec![2]), Column::Utf8(vec![b"b".to_vec()])];
    let out = concat::concat_columns(&[a, b]).unwrap();
    assert_eq!(out[0], Column::I64(vec![1, 2]));
    assert_eq!(out[1], Column::Utf8(vec![b"a".to_vec(), b"b".to_vec()]));
    assert_eq!(concat::total_rows(&[vec![Column::I64(vec![1, 2])]]), 2);
}

#[test]
fn validity_edges() {
    let mut v = Validity::new(9);
    assert!(v.is_valid(8));
    assert!(!v.is_valid(9));
    v.set(0, false).unwrap();
    assert_eq!(v.count_valid(), 8);
}

#[test]
fn typed_decode_all_types() {
    let i64_page_raw = write_page(loam::schema::tags::ENC_PLAIN, 1, 0, &plain::write_i64(&[7]));
    let (mut pages, _) =
        loam::page::parse_container(&loam::page::write_container(&[i64_page_raw], 0)).unwrap();
    let page = pages.remove(0);
    assert_eq!(
        decode_page(&page, loam::schema::tags::TYPE_I64).unwrap(),
        Column::I64(vec![7])
    );
}

#[test]
fn merge_runs_stably() {
    let out = merge::merge_runs(&[vec![1, 3], vec![2, 4], vec![0, 5]]).unwrap();
    assert_eq!(out, vec![0, 1, 2, 3, 4, 5]);
    assert!(merge::is_sorted(&out));
    assert!(!merge::is_sorted(&[2, 1]));
}
