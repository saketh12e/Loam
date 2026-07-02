//! Durability and recovery-shaped coverage: footers, checksums, truncation.

use loam::error::Error;
use loam::format::{self, FileWriter, ZoneMap};
use loam::page::write_page;
use loam::schema::tags;

fn sample() -> Vec<u8> {
    let mut w = FileWriter::new(format::VERSION_V1);
    w.column(1, "a", tags::TYPE_I64, tags::ENC_PLAIN, false);
    let page = write_page(tags::ENC_PLAIN, 2, 0, &[0u8; 16]);
    w.row_group(
        2,
        vec![(page, 2, tags::ENC_PLAIN)],
        ZoneMap {
            min: 0,
            max: 2,
            rows: 2,
        },
    );
    w.finish()
}

#[test]
fn truncated_file_is_rejected() {
    let bytes = sample();
    for cut in [4, 10, bytes.len() / 2, bytes.len() - 2] {
        assert!(format::open(&bytes[..cut]).is_err(), "cut at {cut}");
    }
}

#[test]
fn corrupted_footer_is_rejected() {
    let mut bytes = sample();
    let n = bytes.len();
    bytes[n - 3] ^= 0x55;
    assert!(matches!(format::open(&bytes), Err(Error::Checksum { .. })));
}

#[test]
fn v1_and_v2_both_open() {
    for version in [format::VERSION_V1, format::VERSION_V2] {
        let mut w = FileWriter::new(version);
        w.column(1, "a", tags::TYPE_I64, tags::ENC_PLAIN, false);
        let page = write_page(tags::ENC_PLAIN, 1, 0, &[0u8; 8]);
        w.row_group(
            1,
            vec![(page, 1, tags::ENC_PLAIN)],
            ZoneMap {
                min: 0,
                max: 1,
                rows: 1,
            },
        );
        let bytes = w.finish();
        assert!(format::open(&bytes).is_ok(), "version {version}");
    }
}
