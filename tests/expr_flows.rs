//! Expression language coverage: tokenizer edges, precedence, folding,
//! relaxation, case expressions, and the eval container codec.

use loam::expr::{self, case, fold};

#[test]
fn comparison_operator_table() {
    for (text, value, want) in [
        ("a > 3", 4, true),
        ("a > 3", 3, false),
        ("a >= 3", 3, true),
        ("a < 3", 2, true),
        ("a <= 3", 3, true),
        ("a == 3", 3, true),
        ("a != 3", 4, true),
    ] {
        let ast = expr::parse(text).unwrap();
        assert_eq!(expr::eval(&ast, value), want, "{text} @ {value}");
    }
}

#[test]
fn and_binds_tighter_than_or() {
    let ast = expr::parse("a == 1 or a == 2 and a == 3").unwrap();
    // Parses as `a == 1 or (a == 2 and a == 3)`.
    assert!(expr::eval(&ast, 1));
    assert!(!expr::eval(&ast, 2));
}

#[test]
fn negative_literals() {
    let ast = expr::parse("a < -5").unwrap();
    assert!(expr::eval(&ast, -10));
    assert!(!expr::eval(&ast, 0));
}

#[test]
fn fold_merges_and_detects() {
    let merged = fold::fold(&expr::parse("a >= 2 and a >= 9").unwrap());
    assert!(expr::eval(&merged, 9));
    assert!(!expr::eval(&merged, 5));
    assert!(fold::never_true(&expr::parse("a > 8 and a < 2").unwrap()));
}

#[test]
fn relax_expands_to_zone() {
    let ast = expr::parse("a > 10 and a < 20").unwrap();
    let relaxed = expr::relax_to_zone(&ast, 0, 100);
    assert!(expr::eval(&relaxed, 5));
    assert!(expr::eval(&relaxed, 50));
    assert!(!expr::eval(&relaxed, 150));
}

#[test]
fn case_with_stream_and_none() {
    let c = case::parse("case when a > 100 then 9 end").unwrap();
    assert_eq!(case::eval(&c, 200), Some(9));
    assert_eq!(case::eval(&c, 1), None);
}

#[test]
fn eval_container_codec_edges() {
    let bytes = expr::write_eval_input("a >= 0", &[0, 1, 2], None);
    let (text, values, late) = expr::parse_eval_input(&bytes).unwrap();
    assert_eq!(text, "a >= 0");
    assert_eq!(values.len(), 3);
    assert!(late.is_none());
    let (.., late2) = expr::parse_eval_input(&expr::write_eval_input(
        "a >= 0",
        &[1],
        Some((-10, 10, 250_000)),
    ))
    .unwrap();
    let summary = late2.unwrap();
    assert_eq!(summary.stats.min, -10);
    assert_eq!(summary.stats.selectivity_ppm, 250_000);
}

#[test]
fn tokenizer_rejects_bad_tokens() {
    assert!(expr::parse("a ! 3").is_err());
    assert!(expr::parse("a >< 3").is_err());
    assert!(expr::parse("a > three").is_err());
    assert!(expr::parse("1 > 2").is_err());
}
