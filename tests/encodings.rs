//! Kernel round-trips across every page encoding.

use loam::batch::decode::{decode_page, Column};
use loam::page::{delta, dict, hybrid, plain, rle, write_container, write_page, ContainerPage};
use loam::schema::tags;

fn one(enc: u32, rows: u32, flags: u8, payload: Vec<u8>) -> ContainerPage {
    let raw = write_page(enc, rows, flags, &payload);
    let (mut pages, _) = loam::page::parse_container(&write_container(&[raw], 0)).unwrap();
    pages.remove(0)
}

#[test]
fn plain_i64_round_trip() {
    let page = one(tags::ENC_PLAIN, 4, 0, plain::write_i64(&[3, 1, 4, 1]));
    assert_eq!(
        decode_page(&page, tags::TYPE_I64).unwrap(),
        Column::I64(vec![3, 1, 4, 1])
    );
}

#[test]
fn plain_f64_round_trip() {
    let page = one(tags::ENC_PLAIN, 2, 0, plain::write_f64(&[2.5, -0.75]));
    assert_eq!(
        decode_page(&page, tags::TYPE_F64).unwrap(),
        Column::F64(vec![2.5, -0.75])
    );
}

#[test]
fn plain_utf8_round_trip() {
    let page = one(
        tags::ENC_PLAIN,
        3,
        0,
        plain::write_utf8(&[b"a", b"bb", b"ccc"]),
    );
    assert_eq!(decode_page(&page, tags::TYPE_UTF8).unwrap().rows(), 3);
}

#[test]
fn delta_round_trip() {
    let page = one(tags::ENC_DELTA, 4, 0, delta::write(&[100, 95, 97, 80]));
    assert_eq!(
        decode_page(&page, tags::TYPE_I64).unwrap(),
        Column::I64(vec![100, 95, 97, 80])
    );
}

#[test]
fn dict_page_round_trip() {
    let payload = dict::write(&[b"red", b"blue"], &[0, 1, 1, 0]);
    let page = one(tags::ENC_DICT, 4, 0, payload);
    match decode_page(&page, tags::TYPE_UTF8).unwrap() {
        Column::Utf8(words) => assert_eq!(words, vec![b"red".to_vec(), b"blue".to_vec()]),
        other => panic!("wrong column {other:?}"),
    }
}

#[test]
fn rle_driver_counts_rows() {
    let page = write_page(
        tags::ENC_RLE,
        9,
        0,
        &rle::write_runs(&[rle::Run { value: 4, len: 5 }, rle::Run { value: 6, len: 4 }]),
    );
    let container = write_container(&[page], 0);
    assert_eq!(loam::page::run_page_driver(&container).unwrap(), 9);
}

#[test]
fn hybrid_driver_counts_rows() {
    let page = write_page(
        tags::ENC_HYBRID,
        5,
        0,
        &hybrid::write(&[b"aa", b"bb"], &[(0, 3), (1, 2)]),
    );
    let container = write_container(&[page], 0);
    assert_eq!(loam::page::run_page_driver(&container).unwrap(), 5);
}
