//! Table catalog: named images with schema fingerprints, used by embedded
//! callers that keep many datasets open at once.

use std::collections::HashMap;

use crate::error::{Error, Result};
use crate::format::FileImage;

/// Stable fingerprint of a schema (column ids, types, encodings in order).
pub fn fingerprint(image: &FileImage<'_>) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for c in image.schema.columns() {
        for b in
            c.id.to_le_bytes()
                .iter()
                .chain([c.type_tag].iter())
                .chain(c.encoding.to_le_bytes().iter())
        {
            h ^= *b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    h
}

struct Entry {
    fingerprint: u64,
    rows: u64,
}

#[derive(Default)]
pub struct Catalog {
    entries: HashMap<String, Entry>,
}

impl Catalog {
    /// Register an image under a name.
    pub fn register(&mut self, name: &str, image: &FileImage<'_>) {
        self.entries.insert(
            name.to_string(),
            Entry {
                fingerprint: fingerprint(image),
                rows: image.zones.iter().map(|z| z.rows as u64).sum(),
            },
        );
    }

    pub fn fingerprint_of(&self, name: &str) -> Result<u64> {
        self.entries
            .get(name)
            .map(|e| e.fingerprint)
            .ok_or(Error::View {
                detail: format!("no table '{name}'"),
            })
    }

    pub fn rows_of(&self, name: &str) -> Result<u64> {
        self.entries.get(name).map(|e| e.rows).ok_or(Error::View {
            detail: format!("no table '{name}'"),
        })
    }

    /// Whether two named tables share a schema fingerprint.
    pub fn compatible(&self, a: &str, b: &str) -> Result<bool> {
        Ok(self.fingerprint_of(a)? == self.fingerprint_of(b)?)
    }

    pub fn tables(&self) -> usize {
        self.entries.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::{FileWriter, ZoneMap};
    use crate::page::{plain, write_page};
    use crate::schema::tags;

    fn image(col: u32) -> Vec<u8> {
        let mut w = FileWriter::new(crate::format::VERSION_V2);
        w.column(col, "a", tags::TYPE_I64, tags::ENC_PLAIN, false);
        let page = write_page(tags::ENC_PLAIN, 2, 0, &plain::write_i64(&[1, 2]));
        w.row_group(
            2,
            vec![(page, 2, tags::ENC_PLAIN)],
            ZoneMap {
                min: 1,
                max: 2,
                rows: 2,
            },
        );
        w.finish()
    }

    #[test]
    fn register_and_compare() {
        let a = image(1);
        let b = image(1);
        let c = image(2);
        let ia = crate::format::open(&a).unwrap();
        let ib = crate::format::open(&b).unwrap();
        let ic = crate::format::open(&c).unwrap();
        let mut cat = Catalog::default();
        cat.register("t1", &ia);
        cat.register("t2", &ib);
        cat.register("t3", &ic);
        assert!(cat.compatible("t1", "t2").unwrap());
        assert!(!cat.compatible("t1", "t3").unwrap());
        assert_eq!(cat.rows_of("t1").unwrap(), 2);
        assert_eq!(cat.tables(), 3);
    }
}
