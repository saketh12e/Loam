//! Statistics stack coverage: histograms, blooms, sketches, quantiles, and
//! prune arithmetic.

use loam::stats::bloom::Bloom;
use loam::stats::histogram::Histogram;
use loam::stats::quantile::Quantile;
use loam::stats::sketch::CountMin;
use loam::stats::{estimate_rows, run_stats_driver, write_stats_input, ColumnStats, LateSummary};

#[test]
fn histogram_tracks_distributions() {
    let values: Vec<i64> = (0..1000).map(|i| (i * 37) % 100).collect();
    let h = Histogram::build(&values, 10).unwrap();
    assert_eq!(h.total(), 1000);
    let est = h.estimate(0, 49);
    assert!(est > 300, "est {est}");
}

#[test]
fn bloom_no_false_negatives_stress() {
    let values: Vec<i64> = (0..500).collect();
    let bloom = Bloom::build(&values);
    for v in &values {
        assert!(bloom.check(*v), "missed {v}");
    }
}

#[test]
fn sketch_heavy_hitters() {
    let mut s = CountMin::default();
    for _ in 0..100 {
        s.add(b"frequent");
    }
    for i in 0..100u16 {
        s.add(&i.to_le_bytes());
    }
    assert!(s.estimate(b"frequent") >= 100);
    for i in 0..100u16 {
        assert!(s.estimate(&i.to_le_bytes()) >= 1);
    }
}

#[test]
fn quantile_reasonable_on_skew() {
    let mut q = Quantile::default();
    for i in 0..1000 {
        q.add(if i % 100 == 0 { 10_000 } else { i });
    }
    let p99 = q.at(0.99).unwrap();
    assert!(p99 >= 900, "p99 {p99}");
}

#[test]
fn estimate_rows_folds_summary() {
    let late = LateSummary {
        column: 1,
        stats: ColumnStats {
            rows_est: 1000,
            min: 0,
            max: 100,
            selectivity_ppm: 500_000,
        },
    };
    assert_eq!(estimate_rows(1000, Some(late)), 500);
    assert_eq!(estimate_rows(1000, None), 1000);
}

#[test]
fn stats_driver_counts_matches() {
    let input = write_stats_input(&[(0, 100, 40), (200, 300, 20), (400, 500, 10)], 50, 250);
    assert_eq!(run_stats_driver(&input).unwrap(), 60);
}
