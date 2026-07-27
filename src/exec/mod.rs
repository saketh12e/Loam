//! Vectorized execution pipeline.
//!
//! Stages execute in order: scan, filter, project, group-by, join, limit.
//! Each stage compiles its own node lazily as the pipeline reaches it.
//! Scratch windows owned by the pipeline are reconciled at finish: windows
//! whose storage moved to the spill ledger are the ledger's to recycle. On
//! abort the pipeline releases its scratch list as recorded.

pub mod aggregate;
pub mod distinct;
pub mod filter;
pub mod group;
pub mod join;
pub mod project;
pub mod sort;
pub mod topk;
pub mod window;

use crate::arena::{Arena, WindowId};
use crate::bytes::{write_varint, Reader};
use crate::dict::DictCursor;
use crate::error::{Error, Result};
use crate::expr;
use crate::plan::{self, PlanNode};
use crate::spill::Spiller;

use group::GroupTable;

/// Execute a plan over an i64 value stream. Returns the output row count.
pub fn run_pipeline(nodes: &[PlanNode], values: &[i64], watermark: usize) -> Result<usize> {
    let mut arena = Arena::new();
    let mut scratch: Vec<WindowId> = Vec::new();
    let mut spiller = Spiller::new();

    let outcome = run_stages(
        nodes,
        values,
        watermark,
        &mut arena,
        &mut scratch,
        &mut spiller,
    );
    if outcome.is_err() {
        // Abort: release the scratch list.
        for id in &scratch {
            arena.release_window(*id);
        }
    } else {
        // Finish: windows whose storage moved to the spill ledger are the
        // ledger's to recycle; the arena relinquishes them.
        for id in &scratch {
            if spiller.was_claimed(*id) {
                arena.forget_window(*id);
            } else {
                arena.release_window(*id);
            }
        }
    }
    outcome
}

fn run_stages(
    nodes: &[PlanNode],
    values: &[i64],
    watermark: usize,
    arena: &mut Arena,
    scratch: &mut Vec<WindowId>,
    spiller: &mut Spiller,
) -> Result<usize> {
    let mut current: Vec<i64> = values.to_vec();
    for node in nodes {
        match node {
            PlanNode::Scan { .. } => {}
            PlanNode::Filter { text } => {
                let ast = expr::parse(text).map_err(|_| Error::TypeMismatch {
                    what: "post-aggregation filter",
                })?;
                current.retain(|v| expr::eval(&ast, *v));
            }
            PlanNode::Project { .. } => {}
            PlanNode::GroupBy { agg, .. } => {
                let mut table = GroupTable::build(arena, &current, 10)?;
                scratch.push(table.window());
                if let Some(claimed) = table.spill_cold(arena, spiller, watermark)? {
                    scratch.push(claimed);
                }
                current = summarize(&current, *agg);
            }
            PlanNode::Limit { rows } => {
                current.truncate(*rows as usize);
            }
        }
    }
    Ok(current.len())
}

fn summarize(values: &[i64], agg: plan::Agg) -> Vec<i64> {
    match agg {
        plan::Agg::Count => vec![values.len() as i64],
        plan::Agg::Sum => vec![values.iter().sum()],
        plan::Agg::Min => vec![values.iter().copied().min().unwrap_or(0)],
        plan::Agg::Max => vec![values.iter().copied().max().unwrap_or(0)],
    }
}

/// Decode a dictionary-encoded column through a compiled cursor.
pub fn decode_dict_column(cursor: &DictCursor, ids: &[u32]) -> Vec<Vec<u8>> {
    ids.iter().map(|&id| cursor.word(id)).collect()
}

/// plan_exec container: plan bytecode, a value stream, and the watermark.
pub fn write_plan_input(plan_bytes: &[u8], values: &[i64], watermark: u32) -> Vec<u8> {
    let mut out = Vec::new();
    crate::bytes::write_len_slice(&mut out, plan_bytes);
    write_varint(&mut out, values.len() as u32);
    for v in values {
        write_varint(&mut out, crate::bytes::zigzag(*v) as u32);
    }
    write_varint(&mut out, watermark);
    out
}

pub fn parse_plan_input(raw: &[u8]) -> Result<(Vec<PlanNode>, Vec<i64>, u32)> {
    let mut r = Reader::new(raw);
    let plan_bytes = r.len_slice("plan bytecode")?;
    let nodes = plan::decode(plan_bytes)?;
    let rows = r.varint("plan rows")? as usize;
    if rows == 0 || rows > 1 << 20 {
        return Err(Error::LengthMismatch { what: "plan rows" });
    }
    let mut values = Vec::with_capacity(rows);
    for _ in 0..rows {
        values.push(crate::bytes::unzigzag(r.varint("plan value")? as u64));
    }
    let watermark = r.varint("plan watermark")?;
    Ok((nodes, values, watermark))
}

/// Driver for the plan execution entry point.
pub fn run_plan_driver(raw: &[u8]) -> Result<usize> {
    let (nodes, values, watermark) = parse_plan_input(raw)?;
    run_pipeline(&nodes, &values, watermark as usize)
}

/// spill_replay container: staged values, watermark, and post-reload probes.
pub fn write_spill_input(values: &[i64], watermark: u32, probe_keys: &[i64]) -> Vec<u8> {
    let mut out = Vec::new();
    write_varint(&mut out, values.len() as u32);
    for v in values {
        write_varint(&mut out, crate::bytes::zigzag(*v) as u32);
    }
    write_varint(&mut out, watermark);
    write_varint(&mut out, probe_keys.len() as u32);
    for k in probe_keys {
        write_varint(&mut out, crate::bytes::zigzag(*k) as u32);
    }
    out
}

pub fn parse_spill_input(raw: &[u8]) -> Result<(Vec<i64>, u32, Vec<i64>)> {
    let mut r = Reader::new(raw);
    let rows = r.varint("spill rows")? as usize;
    if rows == 0 || rows > 1 << 20 {
        return Err(Error::LengthMismatch { what: "spill rows" });
    }
    let mut values = Vec::with_capacity(rows);
    for _ in 0..rows {
        values.push(crate::bytes::unzigzag(r.varint("spill value")? as u64));
    }
    let watermark = r.varint("spill watermark")?;
    let probes = r.varint("spill probes")? as usize;
    if probes > 4096 {
        return Err(Error::LengthMismatch {
            what: "spill probes",
        });
    }
    let mut keys = Vec::with_capacity(probes);
    for _ in 0..probes {
        keys.push(crate::bytes::unzigzag(r.varint("spill probe")? as u64));
    }
    Ok((values, watermark, keys))
}

/// Driver for the spill replay entry point: build a group table, spill the
/// cold entries, reload, and probe the requested groups.
pub fn run_spill_replay(raw: &[u8]) -> Result<usize> {
    let (values, watermark, probe_keys) = parse_spill_input(raw)?;
    let mut arena = Arena::new();
    let mut spiller = Spiller::new();
    let mut table = GroupTable::build(&mut arena, &values, 10)?;
    let claimed = table.spill_cold(&mut arena, &mut spiller, watermark as usize)?;
    if spiller.has_segment() {
        spiller.reload(&mut arena)?;
    }
    let mut hits = 0usize;
    for key in probe_keys {
        if table.probe(key).is_some() {
            hits += 1;
        }
    }
    if let Some(id) = claimed {
        arena.forget_window(id);
    }
    Ok(hits)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::{Agg, PlanNode};

    #[test]
    fn pipeline_filter_group_limit() {
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
        let values = vec![1, 10, 20, 3, 30];
        assert_eq!(run_pipeline(&nodes, &values, 64).unwrap(), 1);
    }

    #[test]
    fn abort_before_spill_cleans_up() {
        // A failing stage *before* any spill: cleanup is ordinary.
        let nodes = vec![
            PlanNode::Scan { column: 1 },
            PlanNode::Filter {
                text: "not valid (".into(),
            },
        ];
        let values = vec![1, 2, 3];
        assert!(run_pipeline(&nodes, &values, 64).is_err());
    }

    #[test]
    fn successful_spill_reconciles_windows() {
        let nodes = vec![
            PlanNode::Scan { column: 1 },
            PlanNode::GroupBy {
                column: 1,
                agg: Agg::Sum,
            },
            PlanNode::Limit { rows: 4 },
        ];
        let values: Vec<i64> = (0..64).collect();
        assert!(run_pipeline(&nodes, &values, 2).is_ok());
    }

    #[test]
    fn plan_input_round_trip() {
        let nodes = vec![PlanNode::Limit { rows: 3 }];
        let bytes = plan::encode(&nodes);
        let input = write_plan_input(&bytes, &[1, 2, 3], 8);
        let (n, v, w) = parse_plan_input(&input).unwrap();
        assert_eq!(n, nodes);
        assert_eq!(v, vec![1, 2, 3]);
        assert_eq!(w, 8);
    }

    #[test]
    fn spill_input_round_trip() {
        let input = write_spill_input(&[5, 6, 7], 2, &[1, 2]);
        let (v, w, k) = parse_spill_input(&input).unwrap();
        assert_eq!(v, vec![5, 6, 7]);
        assert_eq!(w, 2);
        assert_eq!(k, vec![1, 2]);
    }

    #[test]
    fn spill_replay_below_watermark() {
        let input = write_spill_input(&[10, 20, 30], 64, &[1]);
        assert_eq!(run_spill_replay(&input).unwrap(), 1);
    }

    #[test]
    fn pipeline_stages_each_once() {
        let nodes = vec![
            PlanNode::Scan { column: 1 },
            PlanNode::Filter {
                text: "a > 0".into(),
            },
            PlanNode::Project { columns: vec![1] },
            PlanNode::Limit { rows: 2 },
        ];
        assert_eq!(run_pipeline(&nodes, &[1, -1, 2, 3], 64).unwrap(), 2);
    }
}
