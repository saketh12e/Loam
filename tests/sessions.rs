//! Session workflow coverage: ingest/query/refresh/view cycles.

use loam::session::{replay, ScriptWriter};

#[test]
fn read_checksum_tracks_reads() {
    let a = ScriptWriter::new()
        .ingest(1, &[9u8; 16])
        .hold(1)
        .read(0)
        .close()
        .finish();
    let b = ScriptWriter::new()
        .ingest(1, &[9u8; 16])
        .hold(1)
        .read(0)
        .close()
        .finish();
    assert_eq!(
        loam::session::run_session_driver(&a).unwrap(),
        loam::session::run_session_driver(&b).unwrap()
    );
}

#[test]
fn dict_refresh_cycle() {
    let script = ScriptWriter::new()
        .install_dict(1, &[b"north", b"south"])
        .query_dict(1, 3, &[0, 1])
        .refresh_dict(1, 1, &[b"east", b"west"])
        .query_dict(1, 3, &[0])
        .close()
        .finish();
    assert!(loam::session::run_session_driver(&script).is_ok());
}

#[test]
fn script_summary_matches_ops() {
    let script = ScriptWriter::new()
        .ingest(1, &[2u8; 8])
        .ingest(2, &[3u8; 8])
        .hold(1)
        .install_dict(1, &[b"a", b"b"])
        .query_dict(1, 3, &[0, 1])
        .create_view(1, 1, "k > 1")
        .close()
        .finish();
    let summary = replay::summarize(&script).unwrap();
    assert_eq!(summary.ingests, 2);
    assert_eq!(summary.queries, 1);
    assert_eq!(summary.views, 1);
}

#[test]
fn multi_page_views_stay_coherent() {
    let script = ScriptWriter::new()
        .ingest(1, &[1u8; 16])
        .ingest(2, &[2u8; 16])
        .hold(1)
        .hold(2)
        .read(0)
        .read(1)
        .close()
        .finish();
    assert!(loam::session::run_session_driver(&script).is_ok());
}
