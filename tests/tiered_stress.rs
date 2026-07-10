//! Tiered cache stress: batch cycles, pressure curves, and driver flows.

use loam::cache::tiered::{self, TieredCache};

#[test]
fn many_warm_fetch_cycles() {
    let mut c = TieredCache::new(1 << 14);
    for i in 0..64u32 {
        c.warm(i, &[(i % 256) as u8; 128]).unwrap();
        assert!(c.fetch(i).is_some());
    }
    assert!(c.resident() > 0);
}

#[test]
fn batch_warming_under_budget() {
    let ops: Vec<(u8, u32, &[u8])> = vec![
        (0x01, 1, &[1u8; 64]),
        (0x03, 0, &[]),
        (0x04, 2, &[2u8; 64]),
        (0x04, 3, &[3u8; 64]),
        (0x05, 0, &[]),
        (0x02, 1, &[]),
        (0x06, 0, &[]),
    ];
    assert!(tiered::run_tiered_driver(&tiered::write_tiered_script(&ops)).is_ok());
}

#[test]
fn eviction_keeps_hottest_pages() {
    let mut c = TieredCache::new(256);
    c.warm(1, &[1u8; 96]).unwrap();
    c.fetch(1);
    c.fetch(1);
    c.warm(2, &[2u8; 96]).unwrap();
    c.warm(3, &[3u8; 96]).unwrap();
    assert!(c.fetch(1).is_some());
}

#[test]
fn warm_same_page_is_idempotent() {
    let mut c = TieredCache::new(1 << 12);
    c.warm(1, &[9u8; 32]).unwrap();
    c.warm(1, &[9u8; 32]).unwrap();
    assert_eq!(c.resident(), 1);
}

#[test]
fn oversized_warm_rejected() {
    let mut c = TieredCache::new(1 << 12);
    assert!(c.warm(1, &vec![0u8; 1 << 21]).is_err());
    assert!(c.warm(1, &[]).is_err());
}

#[test]
fn reads_after_batch_close() {
    let ops: Vec<(u8, u32, &[u8])> = vec![
        (0x01, 1, &[4u8; 48]),
        (0x02, 1, &[]),
        (0x03, 0, &[]),
        (0x05, 0, &[]),
        (0x06, 0, &[]),
    ];
    assert!(tiered::run_tiered_driver(&tiered::write_tiered_script(&ops)).is_ok());
}
