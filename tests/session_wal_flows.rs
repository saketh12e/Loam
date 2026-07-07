//! Session WAL, checkpoint, and script-summary coverage.

use loam::session::{checkpoint, replay, wal, ScriptWriter};

#[test]
fn wal_sequence_enforced() {
    let mut j = Vec::new();
    wal::append(&mut j, 1, b"a");
    wal::append(&mut j, 2, b"b");
    wal::append(&mut j, 3, b"c");
    let frames = wal::recover(&j).unwrap();
    assert_eq!(frames.len(), 3);
    assert_eq!(frames[0].sequence, 1);
}

#[test]
fn wal_stops_at_corruption() {
    let mut j = Vec::new();
    wal::append(&mut j, 1, b"good");
    let mut tail = Vec::new();
    wal::append(&mut tail, 2, b"bad");
    let n = tail.len();
    tail[n - 5] ^= 0xff; // corrupt the payload, not the crc
    j.extend_from_slice(&tail);
    let frames = wal::recover(&j).unwrap();
    assert_eq!(frames.len(), 1);
}

#[test]
fn checkpoint_codec_edges() {
    let cp = checkpoint::SessionCheckpoint {
        cache_epoch: u32::MAX as u64,
        held_views: 0,
        read_checksum: u64::MAX,
        plans_cached: 1,
    };
    let raw = checkpoint::write_checkpoint(&cp);
    assert_eq!(checkpoint::parse_checkpoint(&raw).unwrap(), cp);
}

#[test]
fn summarize_counts_every_op() {
    let script = ScriptWriter::new()
        .ingest(1, &[1u8; 8])
        .ingest(2, &[2u8; 8])
        .hold(1)
        .fork(0)
        .read(0)
        .install_dict(1, &[b"a"])
        .refresh_dict(1, 1, &[b"b"])
        .query_dict(1, 3, &[0, 1])
        .create_view(1, 1, "k > 0")
        .invalidate_view(1)
        .begin_sweep()
        .close()
        .finish();
    let s = replay::summarize(&script).unwrap();
    assert_eq!(s.ingests, 2);
    assert_eq!(s.refreshes, 1);
    assert_eq!(s.queries, 1);
    assert_eq!(s.views, 1);
    assert!(s.ops >= 10);
}

#[test]
fn summarize_unclosed_script() {
    let script = ScriptWriter::new().ingest(1, &[1u8; 4]).finish();
    let s = replay::summarize(&script).unwrap();
    assert_eq!(s.ingests, 1);
}

#[test]
fn session_chains_multiple_dict_cycles() {
    let script = ScriptWriter::new()
        .install_dict(1, &[b"a"])
        .refresh_dict(1, 0, &[b"b"])
        .refresh_dict(1, 0, &[b"c"])
        .query_dict(1, 3, &[0])
        .close()
        .finish();
    assert!(loam::session::run_session_driver(&script).is_ok());
}
