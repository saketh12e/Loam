//! Malformed-input edges across every public parser: truncation, bad magic,
//! bad counts, and invalid tags must error, never panic or corrupt.

use loam::format::{self, compress, stats_io};
use loam::page::{self, bitpack, delta, dict, hybrid, prefix, rle};
use loam::session::wal;
use loam::spill::{self, partition};

#[test]
fn truncated_inputs_error() {
    assert!(format::open(&[0u8; 3]).is_err());
    assert!(page::parse_container(&[0u8; 1]).is_err());
    assert!(stats_io::read_sidecar(&[0u8; 1]).is_err());
    assert!(spill::read_segment(&[0u8; 1]).is_err());
    assert!(partition::read_partitions(&[0u8; 1], &[]).is_err());
}

#[test]
fn bad_magic_everywhere() {
    assert!(format::open(b"XXXX00000000").is_err());
    assert!(page::parse_container(b"XX").is_err());
    assert!(compress::decompress(b"XX\x00").is_err());
    assert!(stats_io::read_sidecar(b"XX\x00").is_err());
    assert!(spill::read_segment(b"XX\x00").is_err());
    assert!(partition::read_partitions(b"XX\x00", &[]).is_err());
}

#[test]
fn zero_counts_rejected() {
    assert!(rle::parse_runs(&[0]).is_err());
    assert!(dict::parse(&[0]).is_err());
    assert!(hybrid::parse(&[0]).is_err());
    assert!(prefix::decode(&[0], 4).is_err());
}

#[test]
fn oversized_counts_rejected() {
    let mut raw = Vec::new();
    loam::bytes::write_varint(&mut raw, 1 << 22);
    assert!(rle::parse_runs(&raw).is_err());
    assert!(dict::parse(&raw).is_err());
}

#[test]
fn delta_overflow_checked() {
    // Base near i64::MAX, then a large positive step: checked_add must error.
    let mut raw = Vec::new();
    loam::bytes::write_varint64(&mut raw, loam::bytes::zigzag(i64::MAX - 1));
    loam::bytes::write_varint64(&mut raw, loam::bytes::zigzag(100));
    assert!(delta::parse(&raw, 2).is_err());
}

#[test]
fn bitpack_width_bounded() {
    assert!(bitpack::parse(&[0u8; 4], 1, 0).is_err());
    assert!(bitpack::parse(&[0u8; 4], 1, 33).is_err());
}

#[test]
fn wal_gap_and_corruption() {
    let mut j = Vec::new();
    wal::append(&mut j, 2, b"starts-at-two");
    assert!(wal::recover(&j).is_err());
}

#[test]
fn prefix_group_bounds() {
    let mut raw = Vec::new();
    loam::bytes::write_varint(&mut raw, 0);
    assert!(prefix::decode(&raw, 2).is_err());
}

#[test]
fn hybrid_row_cap() {
    let payload = hybrid::write(&[b"a"], &[(0, 1)]);
    assert!(hybrid::expanded_rows(&hybrid::parse(&payload).unwrap()).is_ok());
    let big = hybrid::HybridPage {
        words: vec![b"a".to_vec()],
        runs: vec![(0, 1 << 20)],
    };
    assert!(hybrid::expanded_rows(&big).is_err());
}

#[test]
fn empty_input_everywhere() {
    assert!(format::open(&[]).is_err());
    assert!(page::parse_container(&[]).is_err());
    assert!(compress::decompress(&[]).is_err());
    assert!(stats_io::read_sidecar(&[]).is_err());
    assert!(spill::read_segment(&[]).is_err());
    assert!(partition::read_partitions(&[], &[]).is_err());
    assert!(loam::expr::parse_eval_input(&[]).is_err());
    assert!(loam::exec::parse_plan_input(&[]).is_err());
    assert!(loam::exec::parse_spill_input(&[]).is_err());
    assert!(loam::session::run_session_driver(&[]).is_ok());
}

#[test]
fn varint_overlong_rejected_in_containers() {
    let mut raw = b"PG".to_vec();
    raw.extend_from_slice(&[0xff, 0xff, 0xff, 0xff, 0xff, 0xff]);
    assert!(page::parse_container(&raw).is_err());
}

#[test]
fn plan_bytecode_trailing_garbage_ok_or_err_not_panic() {
    let nodes = vec![loam::plan::PlanNode::Limit { rows: 1 }];
    let mut bytes = loam::plan::encode(&nodes);
    bytes.extend_from_slice(&[0xaa, 0xbb]);
    let _ = loam::plan::decode(&bytes);
}

#[test]
fn dict_segment_id_cap() {
    let seg = loam::dict::write_dict_segment(&[b"a"], &[0]);
    assert!(loam::dict::run_dict_driver(&seg).is_ok());
    let bad = loam::dict::write_dict_segment(&[b"a"], &[7]);
    assert!(loam::dict::run_dict_driver(&bad).is_err());
}
