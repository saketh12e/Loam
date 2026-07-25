//! Query plans, plan bytecode, and the compiled-plan cache.
//!
//! Plans compile against the current dictionary for a column: cursors are
//! bound at compile time and cached per query shape so repeated queries skip
//! recompilation.

pub mod cost;
pub mod explain;
pub mod opt;
pub mod rules;
pub mod schedule;

use std::collections::HashMap;

use crate::bytes::{write_varint, Reader};
use crate::dict::{DictCursor, DictRegistry};
use crate::error::{Error, Result};
use crate::expr::{self, Ast};

#[derive(Debug, Clone, PartialEq)]
pub enum PlanNode {
    Scan { column: u32 },
    Filter { text: String },
    Project { columns: Vec<u32> },
    GroupBy { column: u32, agg: Agg },
    Limit { rows: u32 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Agg {
    Count,
    Sum,
    Min,
    Max,
}

const OP_SCAN: u8 = 1;
const OP_FILTER: u8 = 2;
const OP_PROJECT: u8 = 3;
const OP_GROUPBY: u8 = 4;
const OP_LIMIT: u8 = 5;

/// Serialize a plan to bytecode.
pub fn encode(nodes: &[PlanNode]) -> Vec<u8> {
    let mut out = Vec::new();
    write_varint(&mut out, nodes.len() as u32);
    for node in nodes {
        match node {
            PlanNode::Scan { column } => {
                out.push(OP_SCAN);
                write_varint(&mut out, *column);
            }
            PlanNode::Filter { text } => {
                out.push(OP_FILTER);
                crate::bytes::write_len_slice(&mut out, text.as_bytes());
            }
            PlanNode::Project { columns } => {
                out.push(OP_PROJECT);
                write_varint(&mut out, columns.len() as u32);
                for c in columns {
                    write_varint(&mut out, *c);
                }
            }
            PlanNode::GroupBy { column, agg } => {
                out.push(OP_GROUPBY);
                write_varint(&mut out, *column);
                out.push(match agg {
                    Agg::Count => 1,
                    Agg::Sum => 2,
                    Agg::Min => 3,
                    Agg::Max => 4,
                });
            }
            PlanNode::Limit { rows } => {
                out.push(OP_LIMIT);
                write_varint(&mut out, *rows);
            }
        }
    }
    out
}

/// Parse plan bytecode.
pub fn decode(raw: &[u8]) -> Result<Vec<PlanNode>> {
    let mut r = Reader::new(raw);
    let count = r.varint("plan nodes")? as usize;
    if count == 0 || count > 64 {
        return Err(Error::Plan {
            detail: "node count out of range".into(),
        });
    }
    let mut nodes = Vec::with_capacity(count);
    for _ in 0..count {
        let op = r.u8("plan op")?;
        let node = match op {
            OP_SCAN => PlanNode::Scan {
                column: r.varint("scan column")?,
            },
            OP_FILTER => {
                let text = r.len_slice("filter text")?;
                let text = String::from_utf8(text.to_vec()).map_err(|_| Error::Plan {
                    detail: "filter text is not utf-8".into(),
                })?;
                PlanNode::Filter { text }
            }
            OP_PROJECT => {
                let n = r.varint("project columns")? as usize;
                let mut columns = Vec::with_capacity(n);
                for _ in 0..n {
                    columns.push(r.varint("project column")?);
                }
                PlanNode::Project { columns }
            }
            OP_GROUPBY => {
                let column = r.varint("group column")?;
                let agg = match r.u8("group agg")? {
                    1 => Agg::Count,
                    2 => Agg::Sum,
                    3 => Agg::Min,
                    4 => Agg::Max,
                    other => {
                        return Err(Error::Plan {
                            detail: format!("unknown agg {other}"),
                        })
                    }
                };
                PlanNode::GroupBy { column, agg }
            }
            OP_LIMIT => PlanNode::Limit {
                rows: r.varint("limit rows")?,
            },
            other => {
                return Err(Error::Plan {
                    detail: format!("unknown op {other}"),
                })
            }
        };
        nodes.push(node);
    }
    Ok(nodes)
}

/// A compiled plan: parsed filters plus dictionary cursors bound at compile
/// time for every scanned dictionary column.
pub struct CompiledPlan {
    pub nodes: Vec<PlanNode>,
    pub filters: Vec<Ast>,
    pub cursors: Vec<(u32, DictCursor)>,
}

/// Cache of compiled plans keyed by the query shape: the scanned column and
/// its encoding. The shape fully determines the compiled nodes.
#[derive(Default)]
pub struct PlanCache {
    entries: HashMap<(u32, u32), CompiledPlan>,
}

impl PlanCache {
    pub fn get_or_compile(
        &mut self,
        column: u32,
        encoding: u32,
        nodes: &[PlanNode],
        dicts: &DictRegistry,
    ) -> Result<&CompiledPlan> {
        let key = (column, encoding);
        if let std::collections::hash_map::Entry::Vacant(slot) = self.entries.entry(key) {
            let mut filters = Vec::new();
            for node in nodes {
                if let PlanNode::Filter { text } = node {
                    filters.push(expr::parse(text)?);
                }
            }
            let mut cursors = Vec::new();
            if let Ok(dict) = dicts.current(column) {
                let cursor = unsafe { DictCursor::bind(dict.store()) };
                cursors.push((column, cursor));
            }
            slot.insert(CompiledPlan {
                nodes: nodes.to_vec(),
                filters,
                cursors,
            });
        }
        Ok(self.entries.get(&key).expect("inserted"))
    }

    pub fn cached_plans(&self) -> usize {
        self.entries.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_plan() -> Vec<PlanNode> {
        vec![
            PlanNode::Scan { column: 1 },
            PlanNode::Filter {
                text: "temp > 10".into(),
            },
            PlanNode::GroupBy {
                column: 1,
                agg: Agg::Sum,
            },
            PlanNode::Limit { rows: 100 },
        ]
    }

    #[test]
    fn bytecode_round_trip() {
        let plan = sample_plan();
        let bytes = encode(&plan);
        assert_eq!(decode(&bytes).unwrap(), plan);
    }

    #[test]
    fn rejects_bad_op() {
        let bytes = vec![1u8, 99, 0];
        assert!(decode(&bytes).is_err());
    }

    #[test]
    fn cache_compiles_once() {
        let mut cache = PlanCache::default();
        let dicts = DictRegistry::default();
        let plan = sample_plan();
        cache.get_or_compile(1, 3, &plan, &dicts).unwrap();
        cache.get_or_compile(1, 3, &plan, &dicts).unwrap();
        assert_eq!(cache.cached_plans(), 1);
    }

    #[test]
    fn project_round_trip_many_columns() {
        let nodes = vec![PlanNode::Project {
            columns: (0..40).collect(),
        }];
        assert_eq!(decode(&encode(&nodes)).unwrap(), nodes);
    }
}
