//! Spill, reload, and view lifecycle coverage.

use loam::exec::{run_plan_driver, run_spill_replay, write_plan_input, write_spill_input};
use loam::plan::{self, Agg, PlanNode};
use loam::view::{run_view_driver, write_view_input};

#[test]
fn groupby_spill_and_finish() {
    let nodes = vec![
        PlanNode::Scan { column: 1 },
        PlanNode::GroupBy {
            column: 1,
            agg: Agg::Sum,
        },
    ];
    let values: Vec<i64> = (0..48).collect();
    let input = write_plan_input(&plan::encode(&nodes), &values, 2);
    assert!(run_plan_driver(&input).is_ok());
}

#[test]
fn spill_replay_round_trip() {
    let input = write_spill_input(&[11, 12, 55, 58], 8, &[1, 5]);
    assert_eq!(run_spill_replay(&input).unwrap(), 2);
}

#[test]
fn view_build_and_invalidate() {
    let input = write_view_input(1, &[b"red", b"blue"], &["k > 10", "k < 4"]);
    assert_eq!(run_view_driver(&input).unwrap(), 2);
}
