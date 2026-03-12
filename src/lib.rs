//! loam — embedded columnar analytics engine.
//!
//! The crate is organized around a versioned columnar file format, a set of
//! page decoding kernels, and a vectorized execution pipeline. Memory-sensitive
//! hot paths (page reinterpretation, zero-copy slices, scratch arenas, spill
//! buffers) are kept behind small safe wrappers.

pub mod arena;
pub mod batch;
pub mod bytes;
pub mod cache;
pub mod dict;
pub mod engine;
pub mod engine_admin;
pub mod error;
pub mod exec;
pub mod expr;
pub mod format;
pub mod page;
pub mod plan;
pub mod query;
pub mod schema;
pub mod session;
pub mod spill;
pub mod stats;
pub mod store;
pub mod view;

pub mod diagnostics;
pub mod metrics;

pub use engine::Engine;
pub use error::{Error, Result};
