//! Pipeline stress: long value streams, watermarks, abort paths, and driver
//! containers.

use loam::exec::{
    run_pipeline, run_plan_driver, run_spill_replay, write_plan_input, write_spill_input,
};
use loam::plan::{self, Agg, PlanNode};

#[test]
fn long_stream_filter_group() {
    let nodes = vec![
        PlanNode::Scan { column: 1 },
        PlanNode::Filter {
            text: "a >= 0".into(),
        },
        PlanNode::GroupBy {
            column: 1,
            agg: Agg::Count,
        },
    ];
    let values: Vec<i64> = (0..4096).collect();
    assert_eq!(run_pipeline(&nodes, &values, 1024).unwrap(), 1);
}

#[test]
fn watermark_spill_success_paths() {
    let nodes = vec![
        PlanNode::Scan { column: 1 },
        PlanNode::GroupBy {
            column: 1,
            agg: Agg::Sum,
        },
        PlanNode::Limit { rows: 4 },
    ];
    for wm in [1, 2, 8, 64] {
        let values: Vec<i64> = (0..48).collect();
        assert!(run_pipeline(&nodes, &values, wm).is_ok(), "wm {wm}");
    }
}

#[test]
fn abort_before_group_is_clean() {
    let nodes = vec![
        PlanNode::Scan { column: 1 },
        PlanNode::Filter {
            text: "broken (".into(),
        },
        PlanNode::GroupBy {
            column: 1,
            agg: Agg::Sum,
        },
    ];
    assert!(run_pipeline(&nodes, &[1, 2, 3], 4).is_err());
}

#[test]
fn plan_driver_round_trip_stress() {
    let nodes = vec![
        PlanNode::Scan { column: 1 },
        PlanNode::GroupBy {
            column: 1,
            agg: Agg::Min,
        },
    ];
    let values: Vec<i64> = (0..512).collect();
    let input = write_plan_input(&plan::encode(&nodes), &values, 128);
    assert_eq!(run_plan_driver(&input).unwrap(), 1);
}

#[test]
fn spill_replay_many_probes() {
    let values: Vec<i64> = (0..20).map(|i| i * 10 + 5).collect();
    let probes: Vec<i64> = (0..20).collect();
    let input = write_spill_input(&values, 64, &probes);
    assert_eq!(run_spill_replay(&input).unwrap(), 20);
}

#[test]
fn limit_zero_is_fine() {
    let nodes = vec![PlanNode::Scan { column: 1 }, PlanNode::Limit { rows: 0 }];
    assert_eq!(run_pipeline(&nodes, &[1, 2, 3], 64).unwrap(), 0);
}

#[test]
fn repeated_pipelines_are_independent() {
    let nodes = vec![
        PlanNode::Scan { column: 1 },
        PlanNode::GroupBy {
            column: 1,
            agg: Agg::Count,
        },
    ];
    for _ in 0..10 {
        assert_eq!(run_pipeline(&nodes, &[1, 2, 3], 64).unwrap(), 1);
    }
}

#[test]
fn groupby_with_aggregates_each() {
    let values = vec![5i64, -3, 9, 1, -8];
    for (agg, check) in [
        (Agg::Count, 5i64),
        (Agg::Sum, 4i64),
        (Agg::Min, -8i64),
        (Agg::Max, 9i64),
    ] {
        let nodes = vec![
            PlanNode::Scan { column: 1 },
            PlanNode::GroupBy { column: 1, agg },
        ];
        assert_eq!(run_pipeline(&nodes, &values, 64).unwrap(), 1, "{check}");
    }
}

#[test]
fn spill_replay_probe_misses() {
    let input = write_spill_input(&[10, 20, 30], 64, &[7, 8, 9]);
    assert_eq!(run_spill_replay(&input).unwrap(), 0);
}

#[test]
fn filter_all_out_then_limit() {
    let nodes = vec![
        PlanNode::Scan { column: 1 },
        PlanNode::Filter {
            text: "a > 1000".into(),
        },
        PlanNode::Limit { rows: 10 },
    ];
    assert_eq!(run_pipeline(&nodes, &[1, 2, 3], 64).unwrap(), 0);
}
