//! Transaction stress: many cycles, mixed resolutions, and failure surfaces.

use loam::session::transactions as txn;

fn cycle(ops: &[(u8, &[u8])]) -> loam::Result<u64> {
    txn::run_txn_driver(&txn::write_txn_script(ops))
}

#[test]
fn many_commit_cycles() {
    let mut ops: Vec<(u8, &[u8])> = Vec::new();
    for _ in 0..25 {
        ops.push((txn::ops::BEGIN, &[]));
        ops.push((txn::ops::STAGE, &[7u8; 12]));
        ops.push((txn::ops::COMMIT, &[]));
    }
    assert_eq!(cycle(&ops).unwrap(), 300);
}

#[test]
fn interleaved_commits_and_rollbacks() {
    let mut ops: Vec<(u8, &[u8])> = Vec::new();
    for i in 0..20 {
        ops.push((txn::ops::BEGIN, &[]));
        if i % 2 == 0 {
            ops.push((txn::ops::STAGE, &[1u8; 10]));
            ops.push((txn::ops::COMMIT, &[]));
        } else {
            ops.push((txn::ops::STAGE, &[2u8; 10]));
            ops.push((txn::ops::ROLLBACK, &[]));
        }
    }
    assert_eq!(cycle(&ops).unwrap(), 100);
}

#[test]
fn stage_at_budget_boundary() {
    let ops: Vec<(u8, &[u8])> = vec![
        (txn::ops::BEGIN, &[]),
        (txn::ops::STAGE, &[1u8; 128]),
        (txn::ops::STAGE, &[1u8; 128]),
        (txn::ops::COMMIT, &[]),
    ];
    assert_eq!(cycle(&ops).unwrap(), 256);
}

#[test]
fn commit_outside_txn_errors() {
    assert!(cycle(&[(txn::ops::COMMIT, &[])]).is_err());
}

#[test]
fn rollback_outside_txn_errors() {
    assert!(cycle(&[(txn::ops::ROLLBACK, &[])]).is_err());
}

#[test]
fn stage_outside_txn_errors() {
    assert!(cycle(&[(txn::ops::STAGE, &[1u8; 4])]).is_err());
}

#[test]
fn unknown_op_errors() {
    assert!(txn::run_txn_driver(&[0x77]).is_err());
}
