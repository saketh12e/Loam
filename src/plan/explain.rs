//! Plan explanation: render a plan as an indented operator tree with per
//! stage row estimates from the cost model.

use crate::plan::{cost, PlanNode};

/// One rendered explanation line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExplainLine {
    pub depth: usize,
    pub text: String,
}

/// Render a plan with row estimates threaded through stages.
pub fn explain(nodes: &[PlanNode], input_rows: u64) -> Vec<ExplainLine> {
    let mut out = Vec::new();
    let mut rows = input_rows.max(1);
    for (i, node) in nodes.iter().enumerate() {
        let stage_cost = cost::stage_cost(node, rows);
        let (name, detail) = match node {
            PlanNode::Scan { column } => ("scan", format!("c{column}")),
            PlanNode::Filter { text } => ("filter", text.clone()),
            PlanNode::Project { columns } => ("project", format!("{columns:?}")),
            PlanNode::GroupBy { column, agg } => ("group_by", format!("c{column} {agg:?}")),
            PlanNode::Limit { rows: cap } => ("limit", format!("{cap}")),
        };
        out.push(ExplainLine {
            depth: i,
            text: format!("{name} {detail} [rows~{rows} cost+{stage_cost}]"),
        });
        rows = match node {
            PlanNode::Filter { .. } => (rows / 2).max(1),
            PlanNode::Limit { rows: cap } => rows.min(*cap as u64).max(1),
            _ => rows,
        };
    }
    out
}

/// Flatten to a printable string.
pub fn render(lines: &[ExplainLine]) -> String {
    let mut out = String::new();
    for line in lines {
        out.push_str(&"  ".repeat(line.depth));
        out.push_str(&line.text);
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::Agg;

    #[test]
    fn renders_all_stages() {
        let nodes = vec![
            PlanNode::Scan { column: 1 },
            PlanNode::Filter {
                text: "a > 10".into(),
            },
            PlanNode::GroupBy {
                column: 1,
                agg: Agg::Sum,
            },
            PlanNode::Limit { rows: 5 },
        ];
        let text = render(&explain(&nodes, 1000));
        assert!(text.contains("scan c1"));
        assert!(text.contains("filter a > 10"));
        assert!(text.contains("group_by"));
        assert!(text.contains("limit 5"));
    }
}
