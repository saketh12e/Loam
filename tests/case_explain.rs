//! Case expressions, plan explanation, and optimization rule flows.

use loam::expr::case;
use loam::plan::{self, explain, rules, Agg, PlanNode};

#[test]
fn case_over_stream() {
    let c = case::parse("case when a < 0 then -1 when a == 0 then 0 else 1 end").unwrap();
    let out = case::eval_stream(&c, &[-5, 0, 12]);
    assert_eq!(out, vec![Some(-1), Some(0), Some(1)]);
}

#[test]
fn explain_renders_estimates() {
    let nodes = vec![
        PlanNode::Scan { column: 1 },
        PlanNode::Filter {
            text: "a > 5".into(),
        },
        PlanNode::GroupBy {
            column: 1,
            agg: Agg::Count,
        },
    ];
    let text = explain::render(&explain::explain(&nodes, 800));
    assert!(text.contains("scan c1 [rows~800"));
    assert!(text.contains("filter a > 5 [rows~800"));
}

#[test]
fn rules_reach_fixed_point() {
    let nodes = vec![
        PlanNode::Filter {
            text: "a > 1".into(),
        },
        PlanNode::Scan { column: 1 },
        PlanNode::Limit { rows: 9 },
        PlanNode::Limit { rows: 3 },
    ];
    let out = rules::optimize(&nodes);
    assert_eq!(out.len(), 3);
    assert!(matches!(out[0], PlanNode::Scan { .. }));
    assert!(matches!(out[2], PlanNode::Limit { rows: 3 }));
    let bytecode = plan::encode(&out);
    assert_eq!(plan::decode(&bytecode).unwrap(), out);
}
