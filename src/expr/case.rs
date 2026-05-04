//! CASE expressions: conditional value selection inside pipelines.
//!
//! `case when <pred> then <lit> [when ...] [else <lit>] end` compiles to a
//! branch table evaluated per row, in order.

use crate::error::{Error, Result};
use crate::expr::{self, Ast};

#[derive(Debug, Clone, PartialEq)]
pub struct CaseExpr {
    pub branches: Vec<(Ast, i64)>,
    pub otherwise: Option<i64>,
}

/// Parse a case expression from text.
pub fn parse(text: &str) -> Result<CaseExpr> {
    let body = text
        .trim()
        .strip_prefix("case ")
        .ok_or(Error::Expression {
            detail: "case expression must start with case".into(),
        })?
        .strip_suffix(" end")
        .ok_or(Error::Expression {
            detail: "case expression must end with end".into(),
        })?;
    let mut branches = Vec::new();
    let mut otherwise = None;
    for part in body.split(" when ") {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        if let Some(rest) = part.strip_prefix("else ") {
            otherwise = Some(rest.trim().parse().map_err(|_| Error::Expression {
                detail: "bad else literal".into(),
            })?);
            continue;
        }
        let part = part.strip_prefix("when ").unwrap_or(part);
        // A trailing `else <lit>` may share the final when-part.
        let (part, tail_else) = match part.split_once(" else ") {
            Some((p, e)) => (p, Some(e)),
            None => (part, None),
        };
        if let Some(e) = tail_else {
            otherwise = Some(e.trim().parse().map_err(|_| Error::Expression {
                detail: "bad else literal".into(),
            })?);
        }
        let (pred, lit) = part.split_once(" then ").ok_or(Error::Expression {
            detail: "when needs then".into(),
        })?;
        let ast = expr::parse(pred.trim())?;
        let lit: i64 = lit.trim().parse().map_err(|_| Error::Expression {
            detail: "bad then literal".into(),
        })?;
        branches.push((ast, lit));
    }
    if branches.is_empty() {
        return Err(Error::Expression {
            detail: "case needs at least one branch".into(),
        });
    }
    Ok(CaseExpr {
        branches,
        otherwise,
    })
}

/// Evaluate the case for one row value.
pub fn eval(case: &CaseExpr, value: i64) -> Option<i64> {
    for (ast, lit) in &case.branches {
        if expr::eval(ast, value) {
            return Some(*lit);
        }
    }
    case.otherwise
}

/// Evaluate over a stream.
pub fn eval_stream(case: &CaseExpr, values: &[i64]) -> Vec<Option<i64>> {
    values.iter().map(|v| eval(case, *v)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_matching_branch_wins() {
        let case = parse("case when a > 10 then 1 when a > 5 then 2 else 3 end").unwrap();
        assert_eq!(eval(&case, 20), Some(1));
        assert_eq!(eval(&case, 7), Some(2));
        assert_eq!(eval(&case, 1), Some(3));
    }

    #[test]
    fn no_match_no_else_is_none() {
        let case = parse("case when a > 10 then 1 end").unwrap();
        assert_eq!(eval(&case, 5), None);
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse("case when a > 1").is_err());
        assert!(parse("select 1").is_err());
    }

    #[test]
    fn stream_eval() {
        let case = parse("case when a > 0 then 1 else 0 end").unwrap();
        assert_eq!(
            eval_stream(&case, &[-1, 0, 5]),
            vec![Some(0), Some(0), Some(1)]
        );
    }
}
