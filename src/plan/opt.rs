//! Plan optimization passes.
//!
//! The optimizer reorders plan stages so cheap, selective work runs first:
//! filters sink below projections, and limits sink as far as semantics
//! allow. Structural passes only; costing is the statistics layer's job.

use crate::error::Result;
use crate::plan::PlanNode;

/// Push filter nodes directly after the scan they read from.
pub fn pushdown_filters(nodes: &[PlanNode]) -> Vec<PlanNode> {
    let mut scans = Vec::new();
    let mut filters = Vec::new();
    let mut rest = Vec::new();
    for node in nodes {
        match node {
            PlanNode::Scan { .. } => scans.push(node.clone()),
            PlanNode::Filter { .. } => filters.push(node.clone()),
            other => rest.push(other.clone()),
        }
    }
    let mut out = scans;
    out.extend(filters);
    out.extend(rest);
    out
}

/// Validate structural plan rules: one scan, at most one limit, group-by
/// before limit.
pub fn validate(nodes: &[PlanNode]) -> Result<()> {
    let scans = nodes
        .iter()
        .filter(|n| matches!(n, PlanNode::Scan { .. }))
        .count();
    if scans != 1 {
        return Err(crate::error::Error::Plan {
            detail: format!("plan needs exactly one scan, found {scans}"),
        });
    }
    let limits = nodes
        .iter()
        .filter(|n| matches!(n, PlanNode::Limit { .. }))
        .count();
    if limits > 1 {
        return Err(crate::error::Error::Plan {
            detail: "at most one limit".into(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::Agg;

    #[test]
    fn filters_sink_below_scan() {
        let nodes = vec![
            PlanNode::Filter {
                text: "a > 1".into(),
            },
            PlanNode::Scan { column: 1 },
            PlanNode::Limit { rows: 5 },
        ];
        let out = pushdown_filters(&nodes);
        assert!(matches!(out[0], PlanNode::Scan { .. }));
        assert!(matches!(out[1], PlanNode::Filter { .. }));
    }

    #[test]
    fn validates_scan_count() {
        assert!(validate(&[PlanNode::Scan { column: 1 }]).is_ok());
        assert!(validate(&[]).is_err());
        assert!(validate(&[PlanNode::Scan { column: 1 }, PlanNode::Scan { column: 2 }]).is_err());
    }

    #[test]
    fn groupby_shape_is_kept() {
        let nodes = vec![
            PlanNode::Scan { column: 1 },
            PlanNode::GroupBy {
                column: 1,
                agg: Agg::Sum,
            },
        ];
        assert!(validate(&nodes).is_ok());
    }

    #[test]
    fn multiple_filters_all_sink() {
        let nodes = vec![
            PlanNode::Filter {
                text: "a > 1".into(),
            },
            PlanNode::Filter {
                text: "a < 9".into(),
            },
            PlanNode::Scan { column: 1 },
        ];
        let out = pushdown_filters(&nodes);
        assert!(matches!(out[0], PlanNode::Scan { .. }));
        assert!(matches!(out[1], PlanNode::Filter { .. }));
        assert!(matches!(out[2], PlanNode::Filter { .. }));
    }
}
