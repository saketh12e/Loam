//! End-to-end admin flows: validate, inspect, report, and snapshot.

use loam::format::{self, validate, FileWriter, ZoneMap};
use loam::page::{plain, write_page};
use loam::schema::tags;
use loam::session::meter::Meter;

fn admin_file() -> Vec<u8> {
    let mut w = FileWriter::new(format::VERSION_V2);
    w.column(1, "v", tags::TYPE_I64, tags::ENC_PLAIN, false);
    for g in 0..4 {
        let base = (g * 10) as i64;
        let page = write_page(
            tags::ENC_PLAIN,
            4,
            0,
            &plain::write_i64(&[base, base + 1, base + 2, base + 3]),
        );
        w.row_group(
            4,
            vec![(page, 4, tags::ENC_PLAIN)],
            ZoneMap {
                min: base,
                max: base + 3,
                rows: 4,
            },
        );
    }
    w.finish()
}

#[test]
fn validate_then_inspect() {
    let bytes = admin_file();
    let image = format::open(&bytes).unwrap();
    validate::validate(&image).unwrap();
    let text = format::inspect::render(&format::inspect::inspect(&image));
    assert!(text.contains("group 3"));
}

#[test]
fn admin_report_flow() {
    let bytes = admin_file();
    let mut engine = loam::Engine::new();
    let _ = engine.open_bytes(&bytes).unwrap();
    let report = loam::engine_admin::report(&engine.metrics, &engine.diagnostics);
    assert_eq!(report.metrics.files_opened, 1);
    assert!(report.text.contains("files=1"));
    assert!(report.events > 0);
}

#[test]
fn meter_renders_in_reports() {
    let mut m = Meter::default();
    m.record(12);
    m.record(400);
    assert_eq!(m.total(), 2);
    assert!(m.render().contains("10u:1"));
}

#[test]
fn validate_rejects_overrun_footer() {
    let mut w = FileWriter::new(format::VERSION_V2);
    w.column(1, "v", tags::TYPE_I64, tags::ENC_PLAIN, false);
    let page = write_page(tags::ENC_PLAIN, 8, 0, &plain::write_i64(&[1; 8]));
    w.row_group(
        2,
        vec![(page, 8, tags::ENC_PLAIN)],
        ZoneMap {
            min: 1,
            max: 1,
            rows: 2,
        },
    );
    let bytes = w.finish();
    let image = format::open(&bytes).unwrap();
    assert!(validate::validate(&image).is_err());
}
