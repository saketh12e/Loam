//! Query front-end: parse a query string into a plan and run it over an
//! opened image.
//!
//! The query surface is deliberately small: `select <cols> from <image>
//! where <expr> limit <n>`. It composes the expression language, the pruner,
//! and the execution pipeline rather than adding a separate engine.

use crate::engine::Engine;
use crate::error::{Error, Result};
use crate::expr;
use crate::format::FileImage;

#[derive(Debug, Clone, PartialEq)]
pub struct Query {
    pub columns: Vec<u32>,
    pub filter: Option<expr::Ast>,
    pub limit: Option<u32>,
}

/// Parse a query string.
pub fn parse(text: &str) -> Result<Query> {
    let lower = text.trim().to_lowercase();
    let rest = lower.strip_prefix("select ").ok_or(Error::Expression {
        detail: "query must start with select".into(),
    })?;
    let (cols_part, rest) = rest.split_once(" from ").ok_or(Error::Expression {
        detail: "query needs from".into(),
    })?;
    let mut columns = Vec::new();
    for c in cols_part.split(',') {
        let c = c.trim();
        let id: u32 =
            c.strip_prefix('c')
                .and_then(|s| s.parse().ok())
                .ok_or(Error::Expression {
                    detail: format!("bad column '{c}' (want cN)"),
                })?;
        columns.push(id);
    }
    if columns.is_empty() {
        return Err(Error::Expression {
            detail: "no columns".into(),
        });
    }
    let mut filter = None;
    let mut limit = None;
    let mut tail = rest.trim();
    if let Some((after_from, maybe_limit)) = tail.split_once(" limit ") {
        tail = after_from;
        let n: u32 = maybe_limit.trim().parse().map_err(|_| Error::Expression {
            detail: "bad limit".into(),
        })?;
        limit = Some(n);
    }
    if let Some(idx) = tail.find("where ") {
        let (table, pred) = tail.split_at(idx);
        let _ = table.trim(); // single-image queries; table name is advisory
        filter = Some(expr::parse(pred.trim_start_matches("where ").trim())?);
    }
    Ok(Query {
        columns,
        filter,
        limit,
    })
}

/// Execute a query over an opened image, returning matching row counts per
/// row group (post-prune).
pub fn run(engine: &mut Engine, image: &FileImage<'_>, query: &Query) -> Result<usize> {
    for &c in &query.columns {
        image.schema.column(c)?;
    }
    let (lo, hi) = match &query.filter {
        Some(ast) => bound_of(ast),
        None => (i64::MIN, i64::MAX),
    };
    let ranges = engine.plan_scan(image, lo, hi)?;
    let mut total: usize = ranges.iter().map(|r| r.row_len as usize).sum();
    if let Some(limit) = query.limit {
        total = total.min(limit as usize);
    }
    Ok(total)
}

/// Crude numeric bounds from a predicate for zone pruning (conservative:
/// falls back to full range whenever the shape is not a simple bound).
fn bound_of(ast: &expr::Ast) -> (i64, i64) {
    match ast {
        expr::Ast::Pred { cmp, lit, .. } => match cmp {
            expr::Cmp::Gt => (lit.saturating_add(1), i64::MAX),
            expr::Cmp::Ge => (*lit, i64::MAX),
            expr::Cmp::Lt => (i64::MIN, lit.saturating_sub(1)),
            expr::Cmp::Le => (i64::MIN, *lit),
            _ => (i64::MIN, i64::MAX),
        },
        expr::Ast::And(l, r) => {
            let (llo, lhi) = bound_of(l);
            let (rlo, rhi) = bound_of(r);
            (llo.max(rlo), lhi.min(rhi))
        }
        expr::Ast::Or(_, _) => (i64::MIN, i64::MAX),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::{FileWriter, ZoneMap};
    use crate::page::{plain, write_page};
    use crate::schema::tags;

    #[test]
    fn parses_full_query() {
        let q = parse("select c1, c2 from t where temp > 10 and temp < 90 limit 5").unwrap();
        assert_eq!(q.columns, vec![1, 2]);
        assert!(q.filter.is_some());
        assert_eq!(q.limit, Some(5));
    }

    #[test]
    fn parses_minimal_query() {
        let q = parse("select c1 from t").unwrap();
        assert_eq!(q.columns, vec![1]);
        assert!(q.filter.is_none());
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse("drop table").is_err());
        assert!(parse("select from t").is_err());
    }

    #[test]
    fn bounds_intersect_on_and() {
        let ast = expr::parse("a > 10 and a < 90").unwrap();
        assert_eq!(bound_of(&ast), (11, 89));
    }

    #[test]
    fn runs_over_image() {
        let mut w = FileWriter::new(crate::format::VERSION_V2);
        w.column(1, "temp", tags::TYPE_I64, tags::ENC_PLAIN, false);
        let page = write_page(tags::ENC_PLAIN, 4, 0, &plain::write_i64(&[10, 20, 30, 40]));
        w.row_group(
            4,
            vec![(page, 4, tags::ENC_PLAIN)],
            ZoneMap {
                min: 10,
                max: 40,
                rows: 4,
            },
        );
        let bytes = w.finish();
        let mut engine = Engine::new();
        let image = engine.open_bytes(&bytes).unwrap();
        let q = parse("select c1 from t where temp > 15").unwrap();
        assert_eq!(run(&mut engine, &image, &q).unwrap(), 4);
    }

    #[test]
    fn query_with_whitespace_variants() {
        assert!(parse("  select   c1   from   t  ").is_ok());
        assert!(parse("select c1,c2 from t").is_ok());
    }
}
