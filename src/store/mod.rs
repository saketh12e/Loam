//! Table store: opened file images kept resident for the engine session.

use crate::error::{Error, Result};
use crate::format::FileImage;

pub mod catalog;
pub mod snapshot;

#[derive(Default)]
pub struct Store<'a> {
    images: Vec<FileImage<'a>>,
}

impl<'a> Store<'a> {
    pub fn add(&mut self, image: FileImage<'a>) -> usize {
        self.images.push(image);
        self.images.len() - 1
    }

    pub fn get(&self, index: usize) -> Result<&FileImage<'a>> {
        self.images
            .get(index)
            .ok_or(Error::BadMagic { what: "store slot" })
    }

    pub fn len(&self) -> usize {
        self.images.len()
    }

    pub fn is_empty(&self) -> bool {
        self.images.is_empty()
    }

    /// Total published rows across resident images.
    pub fn total_rows(&self) -> u64 {
        self.images
            .iter()
            .map(|img| img.zones.iter().map(|z| z.rows as u64).sum::<u64>())
            .sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::{FileWriter, ZoneMap};
    use crate::page::write_page;
    use crate::schema::tags;

    #[test]
    fn add_and_total_rows() {
        let mut w = FileWriter::new(crate::format::VERSION_V2);
        w.column(1, "a", tags::TYPE_I64, tags::ENC_PLAIN, false);
        let page = write_page(tags::ENC_PLAIN, 4, 0, &[0u8; 32]);
        w.row_group(
            4,
            vec![(page, 4, tags::ENC_PLAIN)],
            ZoneMap {
                min: 0,
                max: 9,
                rows: 4,
            },
        );
        let bytes = w.finish();
        let img = crate::format::open(&bytes).unwrap();
        let mut store = Store::default();
        let idx = store.add(img);
        assert_eq!(store.len(), 1);
        assert_eq!(store.get(idx).unwrap().zones[0].rows, 4);
        assert_eq!(store.total_rows(), 4);
    }

    #[test]
    fn missing_slot_errors() {
        let store = Store::default();
        assert!(store.get(3).is_err());
    }
}
