//! Session transactions: stage multi-op write batches and apply them
//! atomically. A validation failure mid-transaction rolls the transaction
//! back automatically; callers may also roll back explicitly.
//!
//! Staged bytes live in an arena window until the transaction resolves:
//! commit flushes staged operations and releases the window, rollback
//! discards them and releases the window.

use crate::arena::{Arena, WindowId};
use crate::bytes::{write_len_slice, Reader};
use crate::error::{Error, Result};

/// Transaction states.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TxnState {
    Active,
    #[allow(dead_code)]
    Resolved,
}

pub struct Txn {
    window: WindowId,
    /// Staging storage captured at open; resolution paths recycle it.
    raw: (*mut u8, usize),
    staged: usize,
    state: TxnState,
}

impl Txn {
    unsafe fn dealloc_raw(&self) {
        let (ptr, bytes) = self.raw;
        if !ptr.is_null() {
            drop(Box::from_raw(std::ptr::slice_from_raw_parts_mut(
                ptr, bytes,
            )));
        }
    }
}

pub struct TxnManager {
    arena: Arena,
    max_staged: usize,
}

/// Ops for the transaction driver.
mod txn_ops {
    pub const BEGIN: u8 = 0x01;
    pub const STAGE: u8 = 0x02;
    pub const COMMIT: u8 = 0x03;
    pub const ROLLBACK: u8 = 0x04;
}

impl TxnManager {
    pub fn new(max_staged: usize) -> Self {
        TxnManager {
            arena: Arena::new(),
            max_staged,
        }
    }

    /// Open a transaction with a fresh staging window.
    pub fn begin(&mut self) -> Result<Txn> {
        let window = self.arena.alloc_window(self.max_staged)?;
        let raw = (self.arena.raw(window)?, self.max_staged);
        Ok(Txn {
            window,
            raw,
            staged: 0,
            state: TxnState::Active,
        })
    }

    /// Stage one payload. Fails validation when the staged total exceeds
    /// the transaction's staging budget.
    pub fn stage(&mut self, txn: &mut Txn, bytes: &[u8]) -> Result<()> {
        if txn.state != TxnState::Active {
            return Err(Error::Session {
                detail: "stage on resolved transaction".into(),
            });
        }
        if txn.staged + bytes.len() > self.max_staged {
            // Validation failure: the transaction rolls back on its own.
            self.auto_rollback(txn);
            return Err(Error::BudgetExceeded {
                what: "transaction staging",
            });
        }
        txn.staged += bytes.len();
        Ok(())
    }

    /// Discard an active transaction automatically (validation path).
    fn auto_rollback(&mut self, txn: &mut Txn) {
        self.arena.release_window(txn.window);
        txn.staged = 0;
    }

    /// Apply the staged operations and release the staging window.
    pub fn commit(&mut self, txn: Txn) -> Result<usize> {
        if txn.state != TxnState::Active {
            return Err(Error::Session {
                detail: "commit on resolved transaction".into(),
            });
        }
        self.arena.forget_window(txn.window);
        unsafe { txn.dealloc_raw() };
        Ok(txn.staged)
    }

    /// Discard the staged operations and release the staging window.
    pub fn rollback(&mut self, txn: Txn) {
        self.arena.forget_window(txn.window);
        unsafe { txn.dealloc_raw() };
    }

    pub fn live_windows(&self) -> usize {
        self.arena.live_windows()
    }
}

/// Driver for the transaction entry point.
pub fn run_txn_driver(raw: &[u8]) -> Result<u64> {
    let mut mgr = TxnManager::new(1 << 8);
    let mut txn: Option<Txn> = None;
    let mut applied = 0u64;
    let mut first_err: Option<Error> = None;
    let mut r = Reader::new(raw);
    while !r.is_empty() {
        let op = r.u8("txn op")?;
        match op {
            txn_ops::BEGIN => {
                if txn.is_some() {
                    return Err(Error::Session {
                        detail: "nested transaction".into(),
                    });
                }
                txn = Some(mgr.begin()?);
            }
            txn_ops::STAGE => {
                let bytes = r.len_slice("stage bytes")?;
                let t = txn.as_mut().ok_or(Error::Session {
                    detail: "stage outside transaction".into(),
                })?;
                // Replay drivers run the whole script and surface the first
                // failure at the end.
                if first_err.is_none() {
                    if let Err(e) = mgr.stage(t, bytes) {
                        first_err = Some(e);
                    }
                }
            }
            txn_ops::COMMIT => {
                let t = txn.take().ok_or(Error::Session {
                    detail: "commit outside transaction".into(),
                })?;
                applied += mgr.commit(t)? as u64;
            }
            txn_ops::ROLLBACK => {
                let t = txn.take().ok_or(Error::Session {
                    detail: "rollback outside transaction".into(),
                })?;
                mgr.rollback(t);
            }
            other => {
                return Err(Error::Session {
                    detail: format!("unknown op {other:#x}"),
                })
            }
        }
    }
    if let Some(e) = first_err {
        return Err(e);
    }
    Ok(applied)
}

/// Serialize a transaction driver script.
pub fn write_txn_script(ops: &[(u8, &[u8])]) -> Vec<u8> {
    let mut out = Vec::new();
    for (op, bytes) in ops {
        out.push(*op);
        if *op == txn_ops::STAGE {
            write_len_slice(&mut out, bytes);
        }
    }
    out
}

/// Op constants re-exported for the script writer.
pub mod ops {
    pub const BEGIN: u8 = 0x01;
    pub const STAGE: u8 = 0x02;
    pub const COMMIT: u8 = 0x03;
    pub const ROLLBACK: u8 = 0x04;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commit_applies_staged() {
        let script = write_txn_script(&[
            (ops::BEGIN, &[]),
            (ops::STAGE, &[1u8; 16]),
            (ops::STAGE, &[2u8; 16]),
            (ops::COMMIT, &[]),
        ]);
        assert_eq!(run_txn_driver(&script).unwrap(), 32);
    }

    #[test]
    fn explicit_rollback_discards() {
        let script = write_txn_script(&[
            (ops::BEGIN, &[]),
            (ops::STAGE, &[1u8; 16]),
            (ops::ROLLBACK, &[]),
        ]);
        assert_eq!(run_txn_driver(&script).unwrap(), 0);
    }

    #[test]
    fn failed_validation_only_errors() {
        let script = write_txn_script(&[
            (ops::BEGIN, &[]),
            (ops::STAGE, &[1u8; 200]),
            (ops::STAGE, &[1u8; 200]),
        ]);
        assert!(run_txn_driver(&script).is_err());
    }

    #[test]
    fn windows_reconcile_on_commit() {
        let mut mgr = TxnManager::new(64);
        let mut txn = mgr.begin().unwrap();
        mgr.stage(&mut txn, &[0u8; 8]).unwrap();
        mgr.commit(txn).unwrap();
        assert_eq!(mgr.live_windows(), 0);
    }

    #[test]
    fn empty_commit_applies_zero() {
        let script = write_txn_script(&[(ops::BEGIN, &[]), (ops::COMMIT, &[])]);
        assert_eq!(run_txn_driver(&script).unwrap(), 0);
    }

    #[test]
    fn many_small_stages() {
        let mut ops: Vec<(u8, &[u8])> = vec![(ops::BEGIN, &[])];
        for _ in 0..10 {
            ops.push((ops::STAGE, &[3u8; 8]));
        }
        ops.push((ops::COMMIT, &[]));
        assert_eq!(run_txn_driver(&write_txn_script(&ops)).unwrap(), 80);
    }
}
