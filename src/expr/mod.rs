//! Expression language: tokenizer, parser, compiler, and range relaxation.
//!
//! Expressions are small: comparisons on one column, joined with `and`/`or`.
//! Compilation estimates selectivity up front so the execution kernel can
//! size its selection buffer once. When late column statistics arrive, the
//! optimizer may relax range bounds outward to zone boundaries so the same
//! predicate can drive zone-map pruning.

pub mod case;
pub mod fold;

use crate::bytes::{write_len_slice, write_varint, Reader};
use crate::error::{Error, Result};
use crate::stats::LateSummary;

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Ident(String),
    Int(i64),
    Gt,
    Lt,
    Ge,
    Le,
    Eq,
    Ne,
    And,
    Or,
}

fn tokenize(text: &str) -> Result<Vec<Tok>> {
    let mut out = Vec::new();
    let mut chars = text.chars().peekable();
    while let Some(&c) = chars.peek() {
        match c {
            ' ' | '\t' | '\n' => {
                chars.next();
            }
            '>' => {
                chars.next();
                if chars.peek() == Some(&'=') {
                    chars.next();
                    out.push(Tok::Ge);
                } else {
                    out.push(Tok::Gt);
                }
            }
            '<' => {
                chars.next();
                if chars.peek() == Some(&'=') {
                    chars.next();
                    out.push(Tok::Le);
                } else {
                    out.push(Tok::Lt);
                }
            }
            '=' => {
                chars.next();
                if chars.peek() == Some(&'=') {
                    chars.next();
                }
                out.push(Tok::Eq);
            }
            '!' => {
                chars.next();
                if chars.peek() == Some(&'=') {
                    chars.next();
                    out.push(Tok::Ne);
                } else {
                    return Err(Error::Expression {
                        detail: "lone '!'".into(),
                    });
                }
            }
            '-' | '0'..='9' => {
                let mut s = String::new();
                s.push(c);
                chars.next();
                while let Some(&d) = chars.peek() {
                    if d.is_ascii_digit() {
                        s.push(d);
                        chars.next();
                    } else {
                        break;
                    }
                }
                let v: i64 = s.parse().map_err(|_| Error::Expression {
                    detail: format!("bad integer '{s}'"),
                })?;
                out.push(Tok::Int(v));
            }
            'a'..='z' | 'A'..='Z' | '_' => {
                let mut s = String::new();
                while let Some(&d) = chars.peek() {
                    if d.is_alphanumeric() || d == '_' {
                        s.push(d);
                        chars.next();
                    } else {
                        break;
                    }
                }
                match s.as_str() {
                    "and" => out.push(Tok::And),
                    "or" => out.push(Tok::Or),
                    _ => out.push(Tok::Ident(s)),
                }
            }
            other => {
                return Err(Error::Expression {
                    detail: format!("unexpected character '{other}'"),
                })
            }
        }
    }
    Ok(out)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cmp {
    Gt,
    Lt,
    Ge,
    Le,
    Eq,
    Ne,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Ast {
    Pred { col: String, cmp: Cmp, lit: i64 },
    And(Box<Ast>, Box<Ast>),
    Or(Box<Ast>, Box<Ast>),
}

struct Parser {
    toks: Vec<Tok>,
    pos: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos)
    }

    fn next(&mut self) -> Option<Tok> {
        let t = self.toks.get(self.pos).cloned();
        if t.is_some() {
            self.pos += 1;
        }
        t
    }

    fn parse_or(&mut self) -> Result<Ast> {
        let mut lhs = self.parse_and()?;
        while matches!(self.peek(), Some(Tok::Or)) {
            self.next();
            let rhs = self.parse_and()?;
            lhs = Ast::Or(Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn parse_and(&mut self) -> Result<Ast> {
        let mut lhs = self.parse_pred()?;
        while matches!(self.peek(), Some(Tok::And)) {
            self.next();
            let rhs = self.parse_pred()?;
            lhs = Ast::And(Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn parse_pred(&mut self) -> Result<Ast> {
        let col = match self.next() {
            Some(Tok::Ident(s)) => s,
            other => {
                return Err(Error::Expression {
                    detail: format!("expected column, found {other:?}"),
                })
            }
        };
        let cmp = match self.next() {
            Some(Tok::Gt) => Cmp::Gt,
            Some(Tok::Lt) => Cmp::Lt,
            Some(Tok::Ge) => Cmp::Ge,
            Some(Tok::Le) => Cmp::Le,
            Some(Tok::Eq) => Cmp::Eq,
            Some(Tok::Ne) => Cmp::Ne,
            other => {
                return Err(Error::Expression {
                    detail: format!("expected comparison, found {other:?}"),
                })
            }
        };
        let lit = match self.next() {
            Some(Tok::Int(v)) => v,
            other => {
                return Err(Error::Expression {
                    detail: format!("expected integer, found {other:?}"),
                })
            }
        };
        Ok(Ast::Pred { col, cmp, lit })
    }
}

pub fn parse(text: &str) -> Result<Ast> {
    let toks = tokenize(text)?;
    if toks.is_empty() {
        return Err(Error::Expression {
            detail: "empty expression".into(),
        });
    }
    let mut p = Parser { toks, pos: 0 };
    let ast = p.parse_or()?;
    if p.pos != p.toks.len() {
        return Err(Error::Expression {
            detail: "trailing tokens".into(),
        });
    }
    Ok(ast)
}

/// A compiled predicate plus the selectivity snapshot taken at compile time.
#[derive(Debug, Clone)]
pub struct Compiled {
    pub ast: Ast,
    /// Estimated matching rows per 1024 input rows, from the compile-time
    /// selectivity table.
    pub est_per_1024: u32,
}

fn cmp_selectivity(cmp: Cmp) -> u32 {
    match cmp {
        Cmp::Eq => 64,
        Cmp::Ne => 960,
        Cmp::Gt | Cmp::Lt => 384,
        Cmp::Ge | Cmp::Le => 448,
    }
}

fn estimate(ast: &Ast) -> u32 {
    match ast {
        Ast::Pred { cmp, .. } => cmp_selectivity(*cmp),
        Ast::And(l, r) => (estimate(l) * estimate(r)) / 1024,
        Ast::Or(l, r) => (estimate(l) + estimate(r)).min(1024),
    }
}

pub fn compile(ast: Ast) -> Compiled {
    let est = estimate(&ast).max(1);
    Compiled {
        ast,
        est_per_1024: est,
    }
}

/// Relax range bounds in a predicate chain outward to zone boundaries so the
/// expression can double as a zone-map pruning predicate. Applied when late
/// statistics arrive for the column.
pub fn relax_to_zone(ast: &Ast, zone_min: i64, zone_max: i64) -> Ast {
    match ast {
        Ast::Pred { col, cmp, .. } => {
            let (cmp, lit) = match cmp {
                Cmp::Gt | Cmp::Ge => (Cmp::Ge, zone_min),
                Cmp::Lt | Cmp::Le => (Cmp::Le, zone_max),
                other => (*other, 0),
            };
            Ast::Pred {
                col: col.clone(),
                cmp,
                lit,
            }
        }
        Ast::And(l, r) => Ast::And(
            Box::new(relax_to_zone(l, zone_min, zone_max)),
            Box::new(relax_to_zone(r, zone_min, zone_max)),
        ),
        Ast::Or(l, r) => Ast::Or(
            Box::new(relax_to_zone(l, zone_min, zone_max)),
            Box::new(relax_to_zone(r, zone_min, zone_max)),
        ),
    }
}

/// Evaluate one row value against the AST.
pub fn eval(ast: &Ast, value: i64) -> bool {
    match ast {
        Ast::Pred { cmp, lit, .. } => match cmp {
            Cmp::Gt => value > *lit,
            Cmp::Lt => value < *lit,
            Cmp::Ge => value >= *lit,
            Cmp::Le => value <= *lit,
            Cmp::Eq => value == *lit,
            Cmp::Ne => value != *lit,
        },
        Ast::And(l, r) => eval(l, value) && eval(r, value),
        Ast::Or(l, r) => eval(l, value) || eval(r, value),
    }
}

/// Input container for the eval driver: expression text, an i64 column, and
/// an optional late-stats trailer.
pub fn parse_eval_input(raw: &[u8]) -> Result<(String, Vec<i64>, Option<LateSummary>)> {
    let mut r = Reader::new(raw);
    let text = r.len_slice("expression text")?;
    let text = String::from_utf8(text.to_vec()).map_err(|_| Error::Expression {
        detail: "expression is not utf-8".into(),
    })?;
    let rows = r.varint("eval rows")? as usize;
    if rows == 0 || rows > 1 << 20 {
        return Err(Error::LengthMismatch { what: "eval rows" });
    }
    let mut values = Vec::with_capacity(rows);
    for _ in 0..rows {
        values.push(crate::bytes::unzigzag(r.varint("eval value")? as u64));
    }
    let late = if r.is_empty() {
        None
    } else {
        let flag = r.u8("late stats flag")?;
        if flag == 0 {
            None
        } else {
            let min = crate::bytes::unzigzag(r.varint("late min")? as u64);
            let max = crate::bytes::unzigzag(r.varint("late max")? as u64);
            let ppm = r.varint("late selectivity")?;
            Some(LateSummary {
                column: 1,
                stats: crate::stats::ColumnStats {
                    rows_est: rows as u32,
                    min,
                    max,
                    selectivity_ppm: ppm.min(1_000_000),
                },
            })
        }
    };
    Ok((text, values, late))
}

/// Serialize an eval input container.
pub fn write_eval_input(text: &str, values: &[i64], late: Option<(i64, i64, u32)>) -> Vec<u8> {
    let mut out = Vec::new();
    write_len_slice(&mut out, text.as_bytes());
    write_varint(&mut out, values.len() as u32);
    for v in values {
        write_varint(&mut out, crate::bytes::zigzag(*v) as u32);
    }
    if let Some((min, max, ppm)) = late {
        out.push(1);
        write_varint(&mut out, crate::bytes::zigzag(min) as u32);
        write_varint(&mut out, crate::bytes::zigzag(max) as u32);
        write_varint(&mut out, ppm);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_evaluates() {
        let ast = parse("temp > 30 and temp < 90").unwrap();
        assert!(eval(&ast, 50));
        assert!(!eval(&ast, 10));
        assert!(!eval(&ast, 120));
    }

    #[test]
    fn or_chains() {
        let ast = parse("a == 1 or a == 2").unwrap();
        assert!(eval(&ast, 1));
        assert!(eval(&ast, 2));
        assert!(!eval(&ast, 3));
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse("temp >> 3").is_err());
        assert!(parse("").is_err());
        assert!(parse("temp > ").is_err());
    }

    #[test]
    fn estimates_shrink_on_and() {
        let wide = compile(parse("a > 0").unwrap());
        let narrow = compile(parse("a > 0 and a < 10").unwrap());
        assert!(narrow.est_per_1024 < wide.est_per_1024);
    }

    #[test]
    fn relaxation_widens_bounds() {
        let ast = parse("temp > 30 and temp < 90").unwrap();
        let relaxed = relax_to_zone(&ast, -100, 500);
        assert!(eval(&relaxed, 0));
        assert!(eval(&relaxed, 200));
        assert!(!eval(&relaxed, -200));
    }

    #[test]
    fn eval_input_round_trip() {
        let bytes = write_eval_input("a > 1", &[1, 2, 3], Some((0, 10, 500_000)));
        let (text, values, late) = parse_eval_input(&bytes).unwrap();
        assert_eq!(text, "a > 1");
        assert_eq!(values, vec![1, 2, 3]);
        assert!(late.is_some());
    }

    #[test]
    fn deeply_nested_and_or() {
        let ast = parse("a > 1 and a > 2 and a > 3 or a == 0").unwrap();
        assert!(eval(&ast, 4));
        assert!(eval(&ast, 0));
        assert!(!eval(&ast, 2));
    }
}
