//! Query workflow: build a two-column file, run queries through the
//! front-end, and print inspection output.

use loam::format::{self, FileWriter, ZoneMap};
use loam::page::{plain, write_page};
use loam::query;
use loam::schema::tags;

fn main() -> loam::Result<()> {
    let mut w = FileWriter::new(format::VERSION_V2);
    w.column(1, "temp", tags::TYPE_I64, tags::ENC_PLAIN, false);
    w.column(2, "humidity", tags::TYPE_F64, tags::ENC_PLAIN, false);
    let temps = write_page(tags::ENC_PLAIN, 4, 0, &plain::write_i64(&[12, 19, 23, 31]));
    w.row_group(
        4,
        vec![(temps, 4, tags::ENC_PLAIN)],
        ZoneMap {
            min: 12,
            max: 31,
            rows: 4,
        },
    );
    let bytes = w.finish();

    let mut engine = loam::Engine::new();
    let image = engine.open_bytes(&bytes)?;
    print!(
        "{}",
        format::inspect::render(&format::inspect::inspect(&image))
    );

    let q = query::parse("select c1 from t where temp > 15 limit 10")?;
    let matched = query::run(&mut engine, &image, &q)?;
    println!("matched rows: {matched}");
    Ok(())
}
