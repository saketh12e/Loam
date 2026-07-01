//! Scan throughput measurement (manual timing; not a criterion bench).

use loam::format::{FileWriter, ZoneMap};
use loam::page::write_page;
use loam::schema::tags;

fn main() {
    let mut w = FileWriter::new(loam::format::VERSION_V2);
    w.column(1, "v", tags::TYPE_I64, tags::ENC_PLAIN, false);
    let rows = 4096usize;
    let mut payload = Vec::with_capacity(rows * 8);
    for i in 0..rows {
        payload.extend_from_slice(&(i as i64).to_le_bytes());
    }
    let page = write_page(tags::ENC_PLAIN, rows as u32, 0, &payload);
    w.row_group(
        rows as u32,
        vec![(page, rows as u32, tags::ENC_PLAIN)],
        ZoneMap {
            min: 0,
            max: rows as i64,
            rows: rows as u32,
        },
    );
    let bytes = w.finish();

    let start = std::time::Instant::now();
    let mut engine = loam::Engine::new();
    let image = engine.open_bytes(&bytes).expect("open");
    let ranges = engine.plan_scan(&image, 100, 4000).expect("scan");
    let elapsed = start.elapsed();
    println!(
        "open+plan: {:?} for {} rows ({} ranges)",
        elapsed,
        rows,
        ranges.len()
    );
}
