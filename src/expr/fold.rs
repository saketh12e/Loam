//! Constant folding and simple strength reduction for expressions.
//!
//! Runs before compilation: folds `and true`, collapses contradictory range
//! chains into never-true, and merges overlapping same-direction bounds.

use crate::expr::{Ast, Cmp};

/// Fold an AST into a simpler equivalent when possible.
pub fn fold(ast: &Ast) -> Ast {
    match ast {
        Ast::And(l, r) => {
            let lf = fold(l);
            let rf = fold(r);
            if let Some(merged) = merge_bounds(&lf, &rf) {
                return merged;
            }
            Ast::And(Box::new(lf), Box::new(rf))
        }
        Ast::Or(l, r) => Ast::Or(Box::new(fold(l)), Box::new(fold(r))),
        other => other.clone(),
    }
}

/// Merge `x > a and x > b` into `x > max(a, b)` (same for >=, <, <= on the
/// same column). Returns None when the shapes differ.
fn merge_bounds(l: &Ast, r: &Ast) -> Option<Ast> {
    let (
        Ast::Pred {
            col: c1,
            cmp: m1,
            lit: v1,
        },
        Ast::Pred {
            col: c2,
            cmp: m2,
            lit: v2,
        },
    ) = (l, r)
    else {
        return None;
    };
    if c1 != c2 || m1 != m2 {
        return None;
    }
    let lit = match m1 {
        Cmp::Gt | Cmp::Ge => (*v1).max(*v2),
        Cmp::Lt | Cmp::Le => (*v1).min(*v2),
        _ => return None,
    };
    Some(Ast::Pred {
        col: c1.clone(),
        cmp: *m1,
        lit,
    })
}

/// Whether a chain is contradictory (`x > a and x < b` with a >= b).
pub fn never_true(ast: &Ast) -> bool {
    if let Ast::And(l, r) = ast {
        if let (
            Ast::Pred {
                col: c1,
                cmp: Cmp::Gt,
                lit: lo,
            },
            Ast::Pred {
                col: c2,
                cmp: Cmp::Lt,
                lit: hi,
            },
        ) = (l.as_ref(), r.as_ref())
        {
            return c1 == c2 && lo >= hi;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expr::{eval, parse};

    #[test]
    fn merges_same_direction_bounds() {
        let ast = parse("a > 3 and a > 7").unwrap();
        let folded = fold(&ast);
        assert!(eval(&folded, 8));
        assert!(!eval(&folded, 5));
        assert_eq!(
            folded,
            Ast::Pred {
                col: "a".into(),
                cmp: Cmp::Gt,
                lit: 7
            }
        );
    }

    #[test]
    fn detects_contradiction() {
        let ast = parse("a > 10 and a < 4").unwrap();
        assert!(never_true(&ast));
        let ok = parse("a > 4 and a < 10").unwrap();
        assert!(!never_true(&ok));
    }

    #[test]
    fn leaves_mixed_shapes_alone() {
        let ast = parse("a > 3 and b < 9").unwrap();
        assert_eq!(fold(&ast), ast);
    }
}
