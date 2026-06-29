//! Maintenance workflow: validate files, inspect them, and review telemetry.

use loam::format::{self, validate, FileWriter, ZoneMap};
use loam::page::{plain, write_page};
use loam::schema::tags;

fn main() -> loam::Result<()> {
    let mut w = FileWriter::new(format::VERSION_V2);
    w.column(1, "temp", tags::TYPE_I64, tags::ENC_PLAIN, false);
    for g in 0..3 {
        let base = (g * 100) as i64;
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
    let bytes = w.finish();

    let mut engine = loam::Engine::new();
    let image = engine.open_bytes(&bytes)?;
    validate::validate(&image)?;
    println!("validation: ok ({} checks)", validate::checks().len());
    print!(
        "{}",
        format::inspect::render(&format::inspect::inspect(&image))
    );
    let report = loam::engine_admin::report(&engine.metrics, &engine.diagnostics);
    print!("{}", report.text);
    Ok(())
}
