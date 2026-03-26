//! Record batches, zero-copy page views, and stripe assembly.
//!
//! `View` is the engine's zero-copy read path: it borrows cache pages and
//! carries the pin set that keeps them resident. `StripeBuilder` assembles
//! fixed-stride expansion stripes for hybrid pages.

pub mod builder;
pub mod concat;
pub mod decode;
pub mod merge;
pub mod validity;

use crate::page::hybrid::{self, HybridPage};

/// An owned columnar buffer.
#[derive(Debug, Clone)]
pub struct ColumnBuf {
    pub bytes: Vec<u8>,
    pub rows: usize,
}

/// A batch of columnar values produced by decoders.
#[derive(Debug, Clone, Default)]
pub struct RecordBatch {
    pub columns: Vec<ColumnBuf>,
    pub rows: usize,
}

impl RecordBatch {
    pub fn from_i64s(values: Vec<i64>) -> Self {
        let mut bytes = Vec::with_capacity(values.len() * 8);
        for v in &values {
            bytes.extend_from_slice(&v.to_le_bytes());
        }
        RecordBatch {
            rows: values.len(),
            columns: vec![ColumnBuf {
                bytes,
                rows: values.len(),
            }],
        }
    }

    pub fn i64_column(&self, idx: usize) -> Option<Vec<i64>> {
        let col = self.columns.get(idx)?;
        if col.bytes.len() % 8 != 0 {
            return None;
        }
        Some(
            col.bytes
                .chunks_exact(8)
                .map(|c| i64::from_le_bytes(c.try_into().unwrap()))
                .collect(),
        )
    }
}

/// A page pin: the cache page plus the epoch at which the pin was taken.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pin {
    pub page: u32,
    pub epoch: u64,
}

/// Zero-copy borrow of one cache page plus the pins keeping it resident.
pub struct View {
    ptr: *const u8,
    len: usize,
    pins: Vec<Pin>,
}

impl View {
    /// # Safety
    /// `ptr..ptr+len` must remain readable while the pins are honored.
    pub unsafe fn from_raw(ptr: *const u8, len: usize, pins: Vec<Pin>) -> Self {
        View { ptr, len, pins }
    }

    /// Share this view's page set with a child scan.
    pub fn fork(&self) -> View {
        View {
            ptr: self.ptr,
            len: self.len,
            pins: self.pins.clone(),
        }
    }

    pub fn pins(&self) -> &[Pin] {
        &self.pins
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Copy the viewed bytes out.
    pub fn read(&self) -> Vec<u8> {
        let mut out = vec![0u8; self.len];
        unsafe {
            std::ptr::copy_nonoverlapping(self.ptr, out.as_mut_ptr(), self.len);
        }
        out
    }

    /// First `n` bytes, for cheap peeks.
    pub fn head(&self, n: usize) -> Vec<u8> {
        let n = n.min(self.len);
        let mut out = vec![0u8; n];
        unsafe {
            std::ptr::copy_nonoverlapping(self.ptr, out.as_mut_ptr(), n);
        }
        out
    }
}

/// Assembles fixed-stride stripes from hybrid pages. The backing buffer is
/// sized by `page::hybrid::plan_stripe` and filled word by word.
pub struct StripeBuilder {
    buf: *mut u8,
    cap: usize,
    stride: usize,
    rows: usize,
}

impl StripeBuilder {
    pub fn planned(cap: usize, stride: usize) -> Self {
        let boxed = vec![0u8; cap].into_boxed_slice();
        let buf = Box::into_raw(boxed) as *mut u8;
        StripeBuilder {
            buf,
            cap,
            stride,
            rows: 0,
        }
    }

    pub fn cap(&self) -> usize {
        self.cap
    }

    pub fn rows(&self) -> usize {
        self.rows
    }

    /// Expand every run into the stripe, padding each word to the stride.
    pub fn expand(&mut self, page: &HybridPage) {
        for &(id, len) in &page.runs {
            let word = &page.words[id as usize];
            for _ in 0..len {
                let dst = unsafe { self.buf.add(self.rows * self.stride) };
                unsafe {
                    std::ptr::copy_nonoverlapping(word.as_ptr(), dst, word.len());
                }
                self.rows += 1;
            }
        }
    }

    /// Detach the assembled stripe as an owned buffer.
    pub fn finish(self) -> Vec<u8> {
        unsafe {
            std::slice::from_raw_parts(self.buf, self.rows * self.stride.min(self.cap.max(1)))
        }
        .to_vec()
    }
}

impl Drop for StripeBuilder {
    fn drop(&mut self) {
        if !self.buf.is_null() {
            unsafe {
                drop(Box::from_raw(std::ptr::slice_from_raw_parts_mut(
                    self.buf, self.cap,
                )));
            }
        }
    }
}

/// Convenience: decode a hybrid page into an owned stripe.
pub fn expand_hybrid(page: &HybridPage) -> crate::error::Result<Vec<u8>> {
    let rows = hybrid::expanded_rows(page)?;
    let mut builder =
        StripeBuilder::planned(hybrid::plan_stripe(rows), hybrid::STRIPE_STRIDE as usize);
    builder.expand(page);
    Ok(builder.finish())
}

