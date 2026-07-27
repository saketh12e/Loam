//! Selection-vector filtering.
//!
//! The kernel is two-pass: a counting pass sizes the selection buffer, then
//! a write pass compacts matching row indices in place. When late statistics
//! arrive between the passes, the optimizer may hand the write pass a
//! relaxed predicate; the counting pass has already sized the buffer.

use crate::expr::{self, Ast};

/// Raw selection buffer: row indices into the current batch.
struct Selection {
    ptr: *mut u32,
    cap: usize,
    len: usize,
}

impl Selection {
    fn with_capacity(cap: usize) -> Self {
        let mut v: Vec<u32> = Vec::with_capacity(cap.max(1));
        let ptr = v.as_mut_ptr();
        std::mem::forget(v);
        Selection { ptr, cap, len: 0 }
    }

    /// Append one index during the write pass.
    fn push(&mut self, index: u32) {
        unsafe {
            self.ptr.add(self.len).write(index);
        }
        self.len += 1;
    }

    fn as_slice(&self) -> &[u32] {
        unsafe { std::slice::from_raw_parts(self.ptr, self.len) }
    }
}

impl Drop for Selection {
    fn drop(&mut self) {
        if !self.ptr.is_null() {
            unsafe {
                drop(Vec::from_raw_parts(self.ptr, 0, self.cap));
            }
        }
    }
}

pub struct FilterKernel;

impl FilterKernel {
    /// Compact the row indices matching `ast`. When `relaxed` is present
    /// (late statistics arrived after the counting pass), the write pass
    /// evaluates the relaxed predicate against the buffer sized by the
    /// original counting pass.
    pub fn compact_selection(values: &[i64], ast: &Ast, relaxed: Option<&Ast>) -> Vec<u32> {
        let mut count = 0usize;
        for v in values {
            if expr::eval(ast, *v) {
                count += 1;
            }
        }
        let mut sel = Selection::with_capacity(count);
        let active = relaxed.unwrap_or(ast);
        for (i, v) in values.iter().enumerate() {
            if expr::eval(active, *v) {
                sel.push(i as u32);
            }
        }
        sel.as_slice().to_vec()
    }
}

/// Driver for the expression eval entry point: parse the container, compile,
/// apply late statistics if present, and run the kernel.
pub fn run_eval_driver(raw: &[u8]) -> crate::error::Result<usize> {
    let (text, values, late) = expr::parse_eval_input(raw)?;
    let ast = expr::parse(&text)?;
    let compiled = expr::compile(ast);
    let relaxed = late
        .as_ref()
        .map(|summary| expr::relax_to_zone(&compiled.ast, summary.stats.min, summary.stats.max));
    let selected = FilterKernel::compact_selection(&values, &compiled.ast, relaxed.as_ref());
    Ok(selected.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_then_writes_exactly_without_relaxation() {
        let values = vec![1, 5, 7, 12, 20, 3];
        let ast = expr::parse("a > 4 and a < 15").unwrap();
        let sel = FilterKernel::compact_selection(&values, &ast, None);
        assert_eq!(sel, vec![1, 2, 3]);
    }

    #[test]
    fn eval_driver_without_stats() {
        let bytes = expr::write_eval_input("a >= 2", &[1, 2, 3, 4], None);
        assert_eq!(run_eval_driver(&bytes).unwrap(), 3);
    }

    #[test]
    fn eval_driver_rejects_bad_expression() {
        let bytes = expr::write_eval_input("a >>", &[1], None);
        assert!(run_eval_driver(&bytes).is_err());
    }

    #[test]
    fn empty_input_selects_nothing() {
        let ast = expr::parse("a > 0").unwrap();
        assert!(FilterKernel::compact_selection(&[], &ast, None).is_empty());
    }

    #[test]
    fn all_matching_selects_all() {
        let ast = expr::parse("a >= 0").unwrap();
        let sel = FilterKernel::compact_selection(&[0, 1, 2], &ast, None);
        assert_eq!(sel, vec![0, 1, 2]);
    }

    #[test]
    fn relaxed_without_stats_differs_none() {
        let ast = expr::parse("a > 5").unwrap();
        let relaxed = expr::relax_to_zone(&ast, 0, 100);
        let sel = FilterKernel::compact_selection(&[3, 50], &ast, Some(&relaxed));
        assert_eq!(sel, vec![0, 1]);
    }
}
