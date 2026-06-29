//! Embedded usage: build a file in memory, open it, run a session script.

use loam::format::{FileWriter, ZoneMap};
use loam::page::write_page;
use loam::schema::tags;

fn main() -> loam::Result<()> {
    let mut w = FileWriter::new(loam::format::VERSION_V2);
    w.column(1, "temp", tags::TYPE_I64, tags::ENC_PLAIN, false);
    let mut payload = Vec::new();
    for v in [12i64, 19, 23, 31] {
        payload.extend_from_slice(&v.to_le_bytes());
    }
    let page = write_page(tags::ENC_PLAIN, 4, 0, &payload);
    w.row_group(
        4,
        vec![(page, 4, tags::ENC_PLAIN)],
        ZoneMap {
            min: 12,
            max: 31,
            rows: 4,
        },
    );
    let bytes = w.finish();

    let mut engine = loam::Engine::new();
    let image = engine.open_bytes(&bytes)?;
    let ranges = engine.plan_scan(&image, 15, 40)?;
    println!("matched ranges: {ranges:?}");

    let mut session = engine.session(1 << 16);
    let script = loam::session::ScriptWriter::new()
        .ingest(1, &bytes[..32])
        .hold(1)
        .read(0)
        .close()
        .finish();
    session.run_script(&script)?;
    println!("session checksum: {}", session.read_checksum);
    Ok(())
}
