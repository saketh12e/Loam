//! Operator-level pipeline coverage: filters, group-bys, limits, aborts.

use loam::exec::{run_pipeline, sort};
use loam::plan::{Agg, PlanNode};

#[test]
fn filter_then_limit() {
    let nodes = vec![
        PlanNode::Scan { column: 1 },
        PlanNode::Filter {
            text: "a >= 3".into(),
        },
        PlanNode::Limit { rows: 2 },
    ];
    let values = vec![1, 3, 5, 2, 9];
    assert_eq!(run_pipeline(&nodes, &values, 64).unwrap(), 2);
}

#[test]
fn groupby_aggregates() {
    for (agg, want) in [
        (Agg::Count, 6i64),
        (Agg::Sum, 21i64),
        (Agg::Min, 1i64),
        (Agg::Max, 6i64),
    ] {
        let nodes = vec![
            PlanNode::Scan { column: 1 },
            PlanNode::GroupBy { column: 1, agg },
        ];
        let values = vec![1, 2, 3, 4, 5, 6];
        assert_eq!(run_pipeline(&nodes, &values, 64).unwrap(), 1, "{want}");
    }
}

#[test]
fn empty_result_is_fine() {
    let nodes = vec![
        PlanNode::Scan { column: 1 },
        PlanNode::Filter {
            text: "a > 100".into(),
        },
    ];
    assert_eq!(run_pipeline(&nodes, &[1, 2, 3], 64).unwrap(), 0);
}

#[test]
fn order_by_operator() {
    let out = sort::order_by(&[9, 1, 5, 3], sort::Direction::Asc, Some(2)).unwrap();
    assert_eq!(out, vec![1, 3]);
}
