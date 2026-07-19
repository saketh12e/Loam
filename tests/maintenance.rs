//! Maintenance flows: global dictionaries, checkpoints, prefetch, budgets.

use loam::cache::prefetch;
use loam::dict::global::GlobalDicts;
use loam::session::checkpoint;
use loam::spill::budget::Budget;
use loam::stats::sketch::CountMin;

#[test]
fn global_dictionary_cycle() {
    let mut g = GlobalDicts::default();
    g.install(
        "region",
        vec![b"us".to_vec(), b"eu".to_vec(), b"ap".to_vec()],
    );
    assert_eq!(g.lookup("region", b"ap"), Some(2));
    assert_eq!(g.get("region").unwrap().words(), 3);
}

#[test]
fn checkpoint_round_trip() {
    let cp = checkpoint::SessionCheckpoint {
        cache_epoch: 12,
        held_views: 2,
        read_checksum: 42,
        plans_cached: 5,
    };
    let raw = checkpoint::write_checkpoint(&cp);
    assert_eq!(checkpoint::parse_checkpoint(&raw).unwrap(), cp);
}

#[test]
fn prefetch_advice_bounds_warm_set() {
    assert_eq!(prefetch::advise(12, 4).len(), 4);
    assert!(prefetch::concurrent_warm(2, 1 << 12));
}

#[test]
fn spill_budget_pressures() {
    let mut b = Budget::new(128);
    assert!(!b.account(64));
    assert!(b.account(96));
    b.spilled(96);
    assert_eq!(b.spills, 1);
}

#[test]
fn sketch_estimates_cardinality() {
    let mut s = CountMin::default();
    for _ in 0..50 {
        s.add(b"common");
    }
    for i in 0..50u8 {
        s.add(&[i]);
    }
    assert!(s.estimate(b"common") >= 50);
}
