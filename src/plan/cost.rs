//! Plan costing: relative stage costs used by the optimizer when choosing
//! between equivalent orderings.

use crate::plan::PlanNode;

/// Relative cost weight of a stage (arbitrary units, lower is cheaper).
pub fn stage_cost(node: &PlanNode, input_rows: u64) -> u64 {
    match node {
        PlanNode::Scan { .. } => input_rows * 2,
        PlanNode::Filter { .. } => input_rows,
        PlanNode::Project { columns } => input_rows / (columns.len().max(1) as u64 + 1),
        PlanNode::GroupBy { .. } => input_rows * 4,
        PlanNode::Limit { rows } => (*rows as u64).min(input_rows),
    }
}

/// Total plan cost, threading a shrinking row estimate through stages.
pub fn plan_cost(nodes: &[PlanNode], input_rows: u64) -> u64 {
    let mut rows = input_rows.max(1);
    let mut total = 0;
    for node in nodes {
        total += stage_cost(node, rows);
        rows = match node {
            PlanNode::Filter { .. } => (rows / 2).max(1),
            PlanNode::Limit { rows: cap } => rows.min(*cap as u64).max(1),
            _ => rows,
        };
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::{Agg, PlanNode};

    #[test]
    fn filtered_plans_cost_less_downstream() {
        let plain = vec![
            PlanNode::Scan { column: 1 },
            PlanNode::GroupBy {
                column: 1,
                agg: Agg::Sum,
            },
        ];
        let filtered = vec![
            PlanNode::Scan { column: 1 },
            PlanNode::Filter {
                text: "a > 1".into(),
            },
            PlanNode::GroupBy {
                column: 1,
                agg: Agg::Sum,
            },
        ];
        let cheap = plan_cost(&filtered, 1000);
        let dear = plan_cost(&plain, 1000);
        assert!(cheap < dear, "{cheap} !< {dear}");
    }
}
