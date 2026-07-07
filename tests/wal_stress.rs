//! WAL stress: long journals, repeated tails, and recovery ordering.

use loam::session::wal;

#[test]
fn long_journal_recovers_in_order() {
    let mut j = Vec::new();
    for i in 1..=200u64 {
        wal::append(&mut j, i, format!("op-{i}").as_bytes());
    }
    let frames = wal::recover(&j).unwrap();
    assert_eq!(frames.len(), 200);
    assert_eq!(frames[199].sequence, 200);
    assert_eq!(frames[100].payload, b"op-101");
}

#[test]
fn empty_journal_recovers_empty() {
    assert!(wal::recover(&[]).unwrap().is_empty());
}

#[test]
fn tail_truncations_ignored() {
    let mut j = Vec::new();
    wal::append(&mut j, 1, b"first");
    wal::append(&mut j, 2, b"second");
    let full = j.len();
    for cut in [full - 1, full - 3, full - 8] {
        let frames = wal::recover(&j[..cut]).unwrap();
        assert!(frames.len() <= 2);
    }
}

#[test]
fn payload_sizes_vary() {
    let mut j = Vec::new();
    wal::append(&mut j, 1, &[]);
    wal::append(&mut j, 2, &[0u8; 512]);
    wal::append(&mut j, 3, b"mid");
    let frames = wal::recover(&j).unwrap();
    assert_eq!(frames.len(), 3);
    assert!(frames[0].payload.is_empty());
    assert_eq!(frames[1].payload.len(), 512);
}
