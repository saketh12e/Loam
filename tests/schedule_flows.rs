//! End-to-end scheduler and rules flows over realistic plans.

use loam::plan::{self, cost, rules, schedule, Agg, PlanNode};

fn analytic_plan() -> Vec<PlanNode> {
    vec![
        PlanNode::Filter {
            text: "a > 100".into(),
        },
        PlanNode::Scan { column: 1 },
        PlanNode::GroupBy {
            column: 1,
            agg: Agg::Sum,
        },
        PlanNode::Limit { rows: 100 },
        PlanNode::Limit { rows: 10 },
    ]
}

#[test]
fn optimize_then_schedule() {
    let optimized = rules::optimize(&analytic_plan());
    assert!(matches!(optimized[0], PlanNode::Scan { .. }));
    let stages = schedule::schedule(&optimized);
    assert_eq!(stages.len(), optimized.len());
    assert_eq!(schedule::critical_path(&stages), optimized.len());
}

#[test]
fn cost_drops_after_optimization() {
    // Collapsing duplicate limits strictly reduces cost.
    let nodes = vec![
        PlanNode::Scan { column: 1 },
        PlanNode::Limit { rows: 100 },
        PlanNode::Limit { rows: 10 },
    ];
    let before = cost::plan_cost(&nodes, 10_000);
    let after = cost::plan_cost(&rules::optimize(&nodes), 10_000);
    assert!(after < before, "{after} !< {before}");
}

#[test]
fn optimized_plan_survives_bytecode() {
    let optimized = rules::optimize(&analytic_plan());
    let bytes = plan::encode(&optimized);
    assert_eq!(plan::decode(&bytes).unwrap(), optimized);
}

#[test]
fn schedule_handles_single_stage() {
    let stages = schedule::schedule(&[PlanNode::Scan { column: 1 }]);
    assert_eq!(stages.len(), 1);
    assert_eq!(stages[0].depends_on, None);
}
