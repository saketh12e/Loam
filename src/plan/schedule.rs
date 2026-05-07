//! Stage scheduling: turn a plan into an execution schedule with explicit
//! stage ids, for instrumentation and progress reporting.

use crate::plan::PlanNode;

/// One scheduled stage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stage {
    pub id: usize,
    pub name: &'static str,
    pub depends_on: Option<usize>,
}

/// Linear schedule: each stage depends on the previous one.
pub fn schedule(nodes: &[PlanNode]) -> Vec<Stage> {
    nodes
        .iter()
        .enumerate()
        .map(|(i, node)| {
            let name = match node {
                PlanNode::Scan { .. } => "scan",
                PlanNode::Filter { .. } => "filter",
                PlanNode::Project { .. } => "project",
                PlanNode::GroupBy { .. } => "group_by",
                PlanNode::Limit { .. } => "limit",
            };
            Stage {
                id: i,
                name,
                depends_on: i.checked_sub(1),
            }
        })
        .collect()
}

/// Critical path length (for a linear plan: the plan length).
pub fn critical_path(stages: &[Stage]) -> usize {
    stages.len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::{Agg, PlanNode};

    #[test]
    fn schedules_in_order_with_dependencies() {
        let nodes = vec![
            PlanNode::Scan { column: 1 },
            PlanNode::GroupBy {
                column: 1,
                agg: Agg::Sum,
            },
            PlanNode::Limit { rows: 3 },
        ];
        let stages = schedule(&nodes);
        assert_eq!(stages.len(), 3);
        assert_eq!(stages[1].depends_on, Some(0));
        assert_eq!(stages[2].name, "limit");
        assert_eq!(critical_path(&stages), 3);
    }
}
