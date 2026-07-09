//! Tiered cache and transaction driver flows (normal paths only).

use loam::cache::tiered;
use loam::session::transactions as txn;

#[test]
fn tiered_batch_flow() {
    let script = tiered::write_tiered_script(&[
        (0x01, 1, &[1u8; 64]),
        (0x01, 2, &[2u8; 64]),
        (0x02, 1, &[]),
        (0x03, 0, &[]),
        (0x04, 3, &[3u8; 64]),
        (0x05, 0, &[]),
        (0x06, 0, &[]),
    ]);
    assert!(tiered::run_tiered_driver(&script).is_ok());
}

#[test]
fn txn_commit_flow() {
    let script = txn::write_txn_script(&[
        (txn::ops::BEGIN, &[]),
        (txn::ops::STAGE, &[9u8; 24]),
        (txn::ops::STAGE, &[8u8; 24]),
        (txn::ops::COMMIT, &[]),
    ]);
    assert_eq!(txn::run_txn_driver(&script).unwrap(), 48);
}

#[test]
fn txn_multiple_cycles() {
    let script = txn::write_txn_script(&[
        (txn::ops::BEGIN, &[]),
        (txn::ops::STAGE, &[1u8; 10]),
        (txn::ops::COMMIT, &[]),
        (txn::ops::BEGIN, &[]),
        (txn::ops::STAGE, &[2u8; 10]),
        (txn::ops::ROLLBACK, &[]),
    ]);
    assert_eq!(txn::run_txn_driver(&script).unwrap(), 10);
}

#[test]
fn nested_begin_rejected() {
    let script = txn::write_txn_script(&[(txn::ops::BEGIN, &[]), (txn::ops::BEGIN, &[])]);
    assert!(txn::run_txn_driver(&script).is_err());
}
