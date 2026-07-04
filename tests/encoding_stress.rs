//! Kernel stress: larger inputs through every encoding and the decode driver.

use loam::page::{delta, dict, for_enc, hybrid, plain, rle, write_container, write_page};
use loam::schema::tags;

fn drive(pages: &[Vec<u8>]) -> loam::Result<usize> {
    loam::page::run_page_driver(&write_container(pages, 0))
}

#[test]
fn plain_large_round_trip() {
    let values: Vec<i64> = (0..4096).map(|i| i * 3 - 1000).collect();
    let page = write_page(
        tags::ENC_PLAIN,
        values.len() as u32,
        0,
        &plain::write_i64(&values),
    );
    assert_eq!(drive(&[page]).unwrap(), 4096);
}

#[test]
fn delta_large_round_trip() {
    let values: Vec<i64> = (0..1000).map(|i| 5000 + (i % 17) * 2).collect();
    let decoded = delta::parse(&delta::write(&values), values.len() as u32).unwrap();
    assert_eq!(decoded, values);
}

#[test]
fn for_large_round_trip() {
    let values: Vec<i64> = (0..2048).map(|i| 10_000 + (i % 97)).collect();
    let decoded = for_enc::parse(&for_enc::write(&values), values.len() as u32).unwrap();
    assert_eq!(decoded, values);
}

#[test]
fn rle_large_expansion() {
    let runs: Vec<rle::Run> = (0..200)
        .map(|i| rle::Run {
            value: i % 7,
            len: 25,
        })
        .collect();
    let page = write_page(tags::ENC_RLE, 5000, 0, &rle::write_runs(&runs));
    assert_eq!(drive(&[page]).unwrap(), 5000);
}

#[test]
fn rle_long_two_page_chain() {
    let p1: Vec<rle::Run> = (0..100).map(|i| rle::Run { value: i, len: 2 }).collect();
    let p2: Vec<rle::Run> = (0..50).map(|i| rle::Run { value: i, len: 2 }).collect();
    let page1 = write_page(
        tags::ENC_RLE,
        200,
        loam::page::FLAG_STARTS_RUN,
        &rle::write_runs(&p1),
    );
    let page2 = write_page(
        tags::ENC_RLE,
        100,
        loam::page::FLAG_ENDS_RUN,
        &rle::write_runs(&p2),
    );
    assert_eq!(drive(&[page1, page2]).unwrap(), 300);
}

#[test]
fn dict_large_unique() {
    let words: [&[u8]; 10] = [
        b"w0", b"w1", b"w2", b"w3", b"w4", b"w5", b"w6", b"w7", b"w8", b"w9",
    ];
    let ids: Vec<u32> = (0..500).map(|i| (i % 10) as u32).collect();
    let page = write_page(tags::ENC_DICT, 500, 0, &dict::write(&words, &ids));
    assert_eq!(drive(&[page]).unwrap(), 500);
}

#[test]
fn hybrid_near_capacity_rows() {
    let payload = hybrid::write(&[b"a", b"b"], &[(0, 1000), (1, 20)]);
    let page = write_page(tags::ENC_HYBRID, 1020, 0, &payload);
    assert_eq!(drive(&[page]).unwrap(), 1020);
}

#[test]
fn multi_page_mixed_stress() {
    let pages = vec![
        write_page(tags::ENC_PLAIN, 100, 0, &plain::write_i64(&vec![1; 100])),
        write_page(tags::ENC_DELTA, 100, 0, &delta::write(&vec![7; 100])),
        write_page(
            tags::ENC_RLE,
            100,
            0,
            &rle::write_runs(&[rle::Run { value: 2, len: 100 }]),
        ),
    ];
    assert_eq!(drive(&pages).unwrap(), 300);
}
