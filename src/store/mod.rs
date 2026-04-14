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

