//! Plan layer coverage: bytecode edges, optimization, costing, rules,
//! explanation, and the compiled-plan cache.

use loam::dict::{DictRegistry, Dictionary};
use loam::plan::{self, cost, explain, opt, rules, Agg, PlanCache, PlanNode};

#[test]
fn bytecode_handles_all_nodes() {
    let nodes = vec![
        PlanNode::Scan { column: 3 },
        PlanNode::Filter {
            text: "x != 0".into(),
        },
        PlanNode::Project {
            columns: vec![1, 2, 5],
        },
        PlanNode::GroupBy {
            column: 2,
            agg: Agg::Max,
        },
        PlanNode::Limit { rows: 42 },
    ];
    assert_eq!(plan::decode(&plan::encode(&nodes)).unwrap(), nodes);
}

#[test]
fn bytecode_rejects_empty_and_huge() {
    assert!(plan::decode(&[]).is_err());
    assert!(plan::decode(&[65]).is_err()); // count 65 > limit
}

#[test]
fn pushdown_and_validate() {
    let nodes = vec![
        PlanNode::Filter {
            text: "a > 1".into(),
        },
        PlanNode::Scan { column: 1 },
    ];
    let out = opt::pushdown_filters(&nodes);
    assert!(matches!(out[0], PlanNode::Scan { .. }));
    assert!(opt::validate(&out).is_ok());
    assert!(opt::validate(&[PlanNode::Limit { rows: 1 }, PlanNode::Limit { rows: 2 }]).is_err());
}

#[test]
fn cost_model_orders_stages() {
    let cheap = cost::stage_cost(
        &PlanNode::Filter {
            text: "a > 1".into(),
        },
        1000,
    );
    let dear = cost::stage_cost(
        &PlanNode::GroupBy {
            column: 1,
            agg: Agg::Sum,
        },
        1000,
    );
    assert!(cheap < dear);
}

#[test]
fn rules_collapse_limits_and_sink_filters() {
    let nodes = vec![
        PlanNode::Filter {
            text: "a > 1".into(),
        },
        PlanNode::Scan { column: 1 },
        PlanNode::Limit { rows: 50 },
        PlanNode::Limit { rows: 10 },
    ];
    let out = rules::optimize(&nodes);
    assert_eq!(out.len(), 3);
    assert!(matches!(out[2], PlanNode::Limit { rows: 10 }));
}

#[test]
fn explain_threads_estimates() {
    let nodes = vec![PlanNode::Scan { column: 1 }, PlanNode::Limit { rows: 10 }];
    let lines = explain::explain(&nodes, 500);
    assert_eq!(lines.len(), 2);
    assert!(lines[1].text.contains("limit 10"));
}

#[test]
fn plan_cache_binds_cursors_when_dict_present() {
    let mut dicts = DictRegistry::default();
    dicts.install(Dictionary::new(1, 1, vec![b"a".to_vec(), b"b".to_vec()]));
    let mut cache = PlanCache::default();
    let nodes = vec![
        PlanNode::Scan { column: 1 },
        PlanNode::Filter {
            text: "k > 0".into(),
        },
    ];
    let compiled = cache.get_or_compile(1, 3, &nodes, &dicts).unwrap();
    assert_eq!(compiled.cursors.len(), 1);
    assert_eq!(compiled.filters.len(), 1);
}
