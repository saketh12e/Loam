//! Session stress: long scripts, many cycles, mixed operation families.

use loam::schema::tags;
use loam::session::ScriptWriter;

#[test]
fn long_ingest_read_cycle() {
    let mut w = ScriptWriter::new().ingest(1, &[1u8; 64]);
    for i in 0..20u32 {
        w = w.ingest(2 + i, &[(i as u8); 32]);
    }
    let script = w.hold(1).read(0).close().finish();
    assert!(loam::session::run_session_driver(&script).is_ok());
}

#[test]
fn repeated_dict_refresh_cycles() {
    let mut w = ScriptWriter::new().install_dict(1, &[b"v0"]);
    for i in 1..12u32 {
        let word = format!("v{i}");
        let word_ref: &'static [u8] = Box::leak(word.into_bytes().into_boxed_slice());
        w = w.refresh_dict(1, 0, &[word_ref]);
    }
    let script = w.query_dict(1, tags::ENC_DICT, &[0]).close().finish();
    assert!(loam::session::run_session_driver(&script).is_ok());
}

#[test]
fn view_create_invalidate_cycles() {
    let mut w = ScriptWriter::new().install_dict(1, &[b"a", b"b"]);
    // View ids increment from 1; invalidate each just-created view.
    for i in 0..6u32 {
        w = w.create_view(1, 1, "k > 0").invalidate_view(i + 1);
    }
    let script = w.close().finish();
    assert!(loam::session::run_session_driver(&script).is_ok());
}

#[test]
fn hold_many_pages_and_read_all() {
    let mut w = ScriptWriter::new();
    for i in 1..10u32 {
        w = w.ingest(i, &[(i as u8); 24]).hold(i);
    }
    for i in 0..9u32 {
        w = w.read(i);
    }
    let script = w.close().finish();
    assert!(loam::session::run_session_driver(&script).is_ok());
}

#[test]
fn sweep_then_reopen_like_flow() {
    let script = ScriptWriter::new()
        .install_dict(1, &[b"x"])
        .refresh_dict(1, 0, &[b"y"])
        .begin_sweep()
        .close()
        .finish();
    assert!(loam::session::run_session_driver(&script).is_ok());
}

#[test]
fn malformed_mid_script_aborts_cleanly() {
    let mut script = ScriptWriter::new().ingest(1, &[1u8; 8]).hold(1).finish();
    script.extend_from_slice(&[0xee, 0xee]);
    assert!(loam::session::run_session_driver(&script).is_err());
}

#[test]
fn fork_then_read_parent_and_child() {
    let script = ScriptWriter::new()
        .ingest(1, &[6u8; 40])
        .hold(1)
        .fork(0)
        .fork(1)
        .read(0)
        .read(1)
        .read(2)
        .close()
        .finish();
    assert!(loam::session::run_session_driver(&script).is_ok());
}

#[test]
fn read_missing_after_close_like_flow() {
    let script = ScriptWriter::new().hold(9).finish();
    assert!(loam::session::run_session_driver(&script).is_err());
}

#[test]
fn dict_query_empty_ids() {
    let script = ScriptWriter::new()
        .install_dict(1, &[b"a"])
        .query_dict(1, 3, &[])
        .close()
        .finish();
    assert!(loam::session::run_session_driver(&script).is_ok());
}
