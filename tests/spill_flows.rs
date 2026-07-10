//! Spill stack coverage: segments, partitions, budgets, and reload flows.

use loam::arena::Arena;
use loam::spill::{budget::Budget, partition, read_segment, write_segment, Spiller};

#[test]
fn segment_multi_part_round_trip() {
    let parts: Vec<(u32, Vec<u8>)> = (0..10).map(|i| (i, vec![i as u8; 16])).collect();
    let seg = write_segment(&parts);
    let got = read_segment(&seg).unwrap();
    assert_eq!(got.len(), 10);
    assert_eq!(got[4].0, 4);
}

#[test]
fn partition_selective_reads() {
    let seg = partition::write_partitions(&[
        (0, b"a".to_vec()),
        (1, b"b".to_vec()),
        (2, b"c".to_vec()),
        (3, b"d".to_vec()),
    ]);
    assert_eq!(partition::read_partitions(&seg, &[0, 2]).unwrap().len(), 2);
    assert!(partition::read_partitions(&seg, &[9]).unwrap().is_empty());
}

#[test]
fn partition_assignment_spreads() {
    let mut counts = [0usize; 4];
    for key in 0..100i64 {
        counts[partition::partition_of(key, 4) as usize] += 1;
    }
    assert_eq!(counts, [25, 25, 25, 25]);
}

#[test]
fn budget_accounting_cycles() {
    let mut b = Budget::new(256);
    for _ in 0..3 {
        assert!(!b.account(100));
        assert!(b.account(200));
        b.spilled(250);
    }
    assert_eq!(b.spills, 3);
}

#[test]
fn claim_reload_claim_cycle() {
    let mut arena = Arena::new();
    let mut spiller = Spiller::new();
    for round in 0..3u32 {
        let id = arena.alloc_window(48).unwrap();
        let raw = arena.raw(id).unwrap();
        unsafe {
            *raw = round as u8;
        }
        spiller.claim(&mut arena, id, round).unwrap();
        let parts = spiller.reload(&mut arena).unwrap();
        assert_eq!(parts[0].1[0], round as u8);
        arena.forget_window(id);
    }
    assert_eq!(spiller.ledger.claimed(), 3);
}

#[test]
fn segment_bounds() {
    let mut raw = b"SP".to_vec();
    loam::bytes::write_varint(&mut raw, 5000);
    assert!(read_segment(&raw).is_err());
}
