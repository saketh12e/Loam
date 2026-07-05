//! Compression frame coverage: round-trips, ratios, corruption handling.

use loam::format::compress;
use loam::page::{plain, rle};

#[test]
fn plain_page_compresses_and_returns() {
    let values: Vec<i64> = (0..256).map(|i| i % 8).collect();
    let payload = plain::write_i64(&values);
    let frame = compress::compress(&payload);
    assert!(frame.len() < payload.len());
    assert_eq!(compress::decompress(&frame).unwrap(), payload);
}

#[test]
fn rle_payload_round_trip() {
    let runs: Vec<rle::Run> = (0..50)
        .map(|i| rle::Run {
            value: i % 5,
            len: 3,
        })
        .collect();
    let payload = rle::write_runs(&runs);
    let frame = compress::compress(&payload);
    assert_eq!(compress::decompress(&frame).unwrap(), payload);
}

#[test]
fn worth_compressing_heuristic() {
    let repetitive = vec![7u8; 512];
    assert!(compress::worth_compressing(&repetitive));
    let varied: Vec<u8> = (0..512).map(|i| (i * 131 % 251) as u8).collect();
    let _ = compress::worth_compressing(&varied); // decision is data-dependent
    assert!(!compress::worth_compressing(&[1u8; 16]));
}

#[test]
fn truncated_frame_errors() {
    let data = b"abcdefghabcdefgh".to_vec();
    let frame = compress::compress(&data);
    for cut in [2, 3, frame.len() - 2] {
        assert!(compress::decompress(&frame[..cut]).is_err(), "cut {cut}");
    }
}
