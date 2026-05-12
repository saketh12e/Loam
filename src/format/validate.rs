//! Deep file validation: a strict pass over an opened image that checks
//! everything the lenient open path accepts but tooling should distrust.
//!
//! `open` is deliberately tolerant (it serves the scan hot path); `validate`
//! is the pass admin tooling runs before trusting a file.

use crate::error::{Error, Result};
use crate::format::FileImage;

/// Every check `validate` performs, as a stable code for reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Check {
    SchemaNegotiable,
    GroupsNonEmpty,
    ZoneBoundsOrdered,
    PageRefsInFile,
    FooterRowsPlausible,
}

/// Run the strict pass; returns the first failing check, or `Ok(())`.
pub fn validate(image: &FileImage<'_>) -> Result<()> {
    for col in image.schema.columns() {
        image.schema.negotiate(col).map_err(|_| Error::BadMagic {
            what: "validate: schema not negotiable",
        })?;
    }
    if image.groups.is_empty() {
        return Err(Error::BadMagic {
            what: "validate: no groups",
        });
    }
    for (i, group) in image.groups.iter().enumerate() {
        if group.pages.is_empty() {
            return Err(Error::BadMagic {
                what: "validate: empty group",
            });
        }
        let zone = image.zones[i];
        if zone.min > zone.max {
            return Err(Error::BadMagic {
                what: "validate: zone bounds inverted",
            });
        }
        for page_ref in &group.pages {
            image.page_bytes(page_ref).map_err(|_| Error::BadMagic {
                what: "validate: page ref out of file",
            })?;
        }
        let physical: u32 = group.pages.iter().map(|p| p.rows).sum();
        // Partial tails are legal only for the final group.
        if i + 1 != image.groups.len() && physical != group.footer_rows {
            return Err(Error::BadMagic {
                what: "validate: mid-file row mismatch",
            });
        }
        if physical > group.footer_rows {
            return Err(Error::BadMagic {
                what: "validate: pages overrun footer",
            });
        }
    }
    Ok(())
}

/// The check codes that ran, for audit logs.
pub fn checks() -> &'static [Check] {
    &[
        Check::SchemaNegotiable,
        Check::GroupsNonEmpty,
        Check::ZoneBoundsOrdered,
        Check::PageRefsInFile,
        Check::FooterRowsPlausible,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::{FileWriter, ZoneMap};
    use crate::page::{plain, write_page};
    use crate::schema::tags;

    fn image(version: u16, footer_rows: u32, page_rows: u32) -> Vec<u8> {
        let mut w = FileWriter::new(version);
        w.column(1, "a", tags::TYPE_I64, tags::ENC_PLAIN, false);
        let page = write_page(tags::ENC_PLAIN, page_rows, 0, &plain::write_i64(&[1; 8]));
        w.row_group(
            footer_rows,
            vec![(page, page_rows, tags::ENC_PLAIN)],
            ZoneMap {
                min: 1,
                max: 8,
                rows: footer_rows,
            },
        );
        w.finish()
    }

    #[test]
    fn valid_file_passes() {
        let bytes = image(crate::format::VERSION_V2, 8, 8);
        let img = crate::format::open(&bytes).unwrap();
        assert!(validate(&img).is_ok());
        assert_eq!(checks().len(), 5);
    }

    #[test]
    fn partial_tail_passes() {
        // Footer runs ahead of the only page: legal for the tail group.
        let bytes = image(crate::format::VERSION_V2, 40, 8);
        let img = crate::format::open(&bytes).unwrap();
        assert!(validate(&img).is_ok());
    }

    #[test]
    fn overrun_rejected() {
        let bytes = image(crate::format::VERSION_V2, 4, 8);
        let img = crate::format::open(&bytes).unwrap();
        assert!(validate(&img).is_err());
    }

    #[test]
    fn negotiate_reports_cleanly() {
        let mut w = FileWriter::new(crate::format::VERSION_V2);
        w.column(1, "a", tags::TYPE_I64, tags::ENC_RLE, false);
        let page = write_page(tags::ENC_PLAIN, 1, 0, &plain::write_i64(&[1]));
        w.row_group(
            1,
            vec![(page, 1, tags::ENC_PLAIN)],
            ZoneMap {
                min: 1,
                max: 1,
                rows: 1,
            },
        );
        let bytes = w.finish();
        let img = crate::format::open(&bytes).unwrap();
        for col in img.schema.columns() {
            assert!(matches!(
                img.schema.negotiate(col).unwrap(),
                crate::schema::ColumnPlan::Mapped(_)
            ));
        }
        assert!(validate(&img).is_ok());
    }

    #[test]
    fn empty_zone_range_is_fine() {
        let mut w = FileWriter::new(crate::format::VERSION_V2);
        w.column(1, "a", tags::TYPE_I64, tags::ENC_PLAIN, false);
        let page = write_page(tags::ENC_PLAIN, 1, 0, &plain::write_i64(&[7]));
        w.row_group(
            1,
            vec![(page, 1, tags::ENC_PLAIN)],
            ZoneMap {
                min: 7,
                max: 7,
                rows: 1,
            },
        );
        let bytes = w.finish();
        let img = crate::format::open(&bytes).unwrap();
        assert!(validate(&img).is_ok());
    }
}
