//! Optimization rule registry: named, ordered rewrites the optimizer can
//! apply to a plan, each with an applicability check.

use crate::plan::{opt, PlanNode};

/// A named structural rule.
pub struct Rule {
    pub name: &'static str,
    pub apply: fn(&[PlanNode]) -> Vec<PlanNode>,
}

/// The default rule set, in application order.
pub fn default_rules() -> Vec<Rule> {
    vec![
        Rule {
            name: "pushdown_filters",
            apply: opt::pushdown_filters,
        },
        Rule {
            name: "drop_redundant_limits",
            apply: drop_redundant_limits,
        },
    ]
}

/// Two limits collapse to the tighter one.
pub fn drop_redundant_limits(nodes: &[PlanNode]) -> Vec<PlanNode> {
    let mut out: Vec<PlanNode> = Vec::with_capacity(nodes.len());
    for node in nodes {
        if let PlanNode::Limit { rows } = node {
            if let Some(PlanNode::Limit { rows: prev }) = out.last_mut() {
                *prev = (*prev).min(*rows);
                continue;
            }
        }
        out.push(node.clone());
    }
    out
}

/// Apply the default rules until a fixed point (bounded iterations).
pub fn optimize(nodes: &[PlanNode]) -> Vec<PlanNode> {
    let mut current = nodes.to_vec();
    for _ in 0..4 {
        let before = current.clone();
        for rule in default_rules() {
            current = (rule.apply)(&current);
        }
        if current == before {
            break;
        }
    }
    current
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collapses_limits() {
        let nodes = vec![
            PlanNode::Scan { column: 1 },
            PlanNode::Limit { rows: 10 },
            PlanNode::Limit { rows: 4 },
        ];
        let out = optimize(&nodes);
        let limits: Vec<u32> = out
            .iter()
            .filter_map(|n| match n {
                PlanNode::Limit { rows } => Some(*rows),
                _ => None,
            })
            .collect();
        assert_eq!(limits, vec![4]);
    }

    #[test]
    fn fixed_point_is_stable() {
        let nodes = vec![
            PlanNode::Filter {
                text: "a > 1".into(),
            },
            PlanNode::Scan { column: 1 },
        ];
        let once = optimize(&nodes);
        assert_eq!(once, optimize(&once));
    }
}
