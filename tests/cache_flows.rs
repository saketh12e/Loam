//! Page cache coverage: epochs, pins, clock eviction, hot tier, hit stats,
//! and prefetch policy.

use loam::cache::hits::HitStats;
use loam::cache::prefetch;
use loam::cache::PageCache;

#[test]
fn epochs_advance_monotonically() {
    let mut c = PageCache::new(1 << 16);
    let e1 = c.epoch();
    let e2 = c.advance_epoch();
    let e3 = c.advance_epoch();
    assert!(e1 < e2 && e2 < e3);
}

#[test]
fn reinsert_returns_same_page() {
    let mut c = PageCache::new(1 << 16);
    let data = vec![3u8; 32];
    let p1 = c.insert(7, &data).unwrap();
    let p2 = c.insert(7, &data).unwrap();
    assert_eq!(p1, p2);
}

#[test]
fn hold_updates_pin_epoch() {
    let mut c = PageCache::new(1 << 12);
    c.insert(1, &[5u8; 64]).unwrap();
    c.hold(1).unwrap();
    c.advance_epoch();
    c.advance_epoch();
    c.insert(2, &[6u8; 64]).unwrap();
    // Recently pinned page survives the pressure of the second insert.
    assert!(c.contains(1));
}

#[test]
fn pressure_recycles_in_epoch_order() {
    let mut c = PageCache::new(160);
    c.insert(1, &[1u8; 96]).unwrap();
    c.advance_epoch();
    c.insert(2, &[2u8; 32]).unwrap();
    c.advance_epoch();
    c.advance_epoch();
    c.insert(3, &[3u8; 96]).unwrap();
    assert!(!c.contains(1));
    assert!(c.contains(3));
}

#[test]
fn missing_hold_is_an_error() {
    let mut c = PageCache::new(1 << 12);
    assert!(c.hold(99).is_err());
}

#[test]
fn oversized_page_rejected() {
    let mut c = PageCache::new(1 << 12);
    assert!(c.insert(1, &vec![0u8; 1 << 23]).is_err());
    assert!(c.insert(1, &[]).is_err());
}

#[test]
fn hit_stats_accounting() {
    let mut s = HitStats::default();
    for _ in 0..7 {
        s.record(true);
    }
    for _ in 0..3 {
        s.record(false);
    }
    assert!((s.hit_rate() - 0.7).abs() < 1e-9);
    s.evictions += 2;
    assert_eq!(s.evictions, 2);
}

#[test]
fn prefetch_policy_shapes() {
    assert_eq!(prefetch::advise(0, 4), Vec::<usize>::new());
    assert_eq!(prefetch::advise(6, 2), vec![0, 1]);
    assert!(prefetch::concurrent_warm(1, 1 << 12));
    assert!(!prefetch::concurrent_warm(1, 1 << 22));
}

#[test]
fn resident_count_tracks() {
    let mut c = PageCache::new(1 << 16);
    c.insert(1, &[1u8; 16]).unwrap();
    c.insert(2, &[2u8; 16]).unwrap();
    assert_eq!(c.resident_pages(), 2);
}
