//! The `.loam` columnar file container.
//!
//! Layout: fixed header, section directory, then sections for schema, row
//! group metadata, and a footer carrying zone maps and authoritative row
//! counts. Pages live at absolute offsets referenced from the row group
//! section.
//!
//! The format is versioned. `V1` and `V2` files differ in how column chunks
//! advertise encodings; negotiation lives in `schema`. A trailing row group
//! may be *partial*: the writer is allowed to publish a footer row count that
//! runs ahead of the pages physically present, so long as the missing rows
//! belong to the tail group. Readers reconcile the two at scan time.

pub mod append;
pub mod compress;
pub mod inspect;
pub mod merge;
pub mod migrate;
pub mod stats_io;
pub mod validate;

use crate::bytes::{crc32c, write_len_slice, write_varint, Reader};
use crate::error::{Error, Result};
use crate::schema::{ColumnDesc, Schema};

pub const FILE_MAGIC: &[u8; 4] = b"LOAM";
pub const VERSION_V1: u16 = 1;
pub const VERSION_V2: u16 = 2;

const SEC_SCHEMA: u8 = 1;
const SEC_ROWGROUPS: u8 = 2;
const SEC_FOOTER: u8 = 3;

/// One row group: a set of column pages plus the count the footer asserts.
#[derive(Debug, Clone)]
pub struct RowGroup {
    pub footer_rows: u32,
    pub pages: Vec<PageRef>,
}

/// Absolute location of one encoded page inside the file.
#[derive(Debug, Clone, Copy)]
pub struct PageRef {
    pub offset: u32,
    pub len: u32,
    pub rows: u32,
    pub encoding: u32,
}

/// Zone map for one row group (i64 columns only in this revision).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ZoneMap {
    pub min: i64,
    pub max: i64,
    /// Row count as published by the writer. May run ahead of the pages in
    /// the final group of a file (partial tail rule).
    pub rows: u32,
}

/// A fully parsed file image. Page payloads are borrowed from the input.
#[derive(Debug)]
pub struct FileImage<'a> {
    pub version: u16,
    pub schema: Schema,
    pub groups: Vec<RowGroup>,
    pub zones: Vec<ZoneMap>,
    pub raw: &'a [u8],
}

impl<'a> FileImage<'a> {
    /// Bytes of one referenced page.
    pub fn page_bytes(&self, r: &PageRef) -> Result<&'a [u8]> {
        let start = r.offset as usize;
        let end = start
            .checked_add(r.len as usize)
            .ok_or(Error::LengthMismatch { what: "page ref" })?;
        if end > self.raw.len() {
            return Err(Error::UnexpectedEof {
                what: "page payload",
            });
        }
        Ok(&self.raw[start..end])
    }

    /// Rows the pages of a group actually carry, as opposed to what the
    /// footer asserts.
    pub fn physical_rows(&self, group: usize) -> u32 {
        self.groups[group].pages.iter().map(|p| p.rows).sum()
    }
}

/// Parse a complete `.loam` image.
pub fn open(raw: &[u8]) -> Result<FileImage<'_>> {
    let mut r = Reader::new(raw);
    let magic = r.take(4, "file magic")?;
    if magic != FILE_MAGIC {
        return Err(Error::BadMagic { what: "file" });
    }
    let version = r.u16("file version")?;
    if version != VERSION_V1 && version != VERSION_V2 {
        return Err(Error::BadMagic {
            what: "file version",
        });
    }
    let sections = r.u16("section count")? as usize;
    if sections == 0 || sections > 8 {
        return Err(Error::LengthMismatch {
            what: "section directory",
        });
    }
    let mut dir = Vec::with_capacity(sections);
    for _ in 0..sections {
        let kind = r.u8("section kind")?;
        let offset = r.u32("section offset")? as usize;
        let len = r.u32("section length")? as usize;
        if offset.checked_add(len).is_none_or(|e| e > raw.len()) {
            return Err(Error::UnexpectedEof {
                what: "section body",
            });
        }
        dir.push((kind, offset, len));
    }

    let mut schema = None;
    let mut groups = None;
    let mut footer = None;
    for (kind, offset, len) in &dir {
        let body = &raw[*offset..*offset + *len];
        match *kind {
            SEC_SCHEMA => schema = Some(parse_schema(body, version)?),
            SEC_ROWGROUPS => groups = Some(parse_rowgroups(body)?),
            SEC_FOOTER => footer = Some(parse_footer(body)?),
            _ => {}
        }
    }
    let schema = schema.ok_or(Error::BadMagic {
        what: "schema section",
    })?;
    let groups = groups.ok_or(Error::BadMagic {
        what: "row group section",
    })?;
    let zones = footer.ok_or(Error::BadMagic {
        what: "footer section",
    })?;
    if zones.len() != groups.len() {
        return Err(Error::LengthMismatch {
            what: "zone map count",
        });
    }
    Ok(FileImage {
        version,
        schema,
        groups,
        zones,
        raw,
    })
}

fn parse_schema(body: &[u8], version: u16) -> Result<Schema> {
    let mut r = Reader::new(body);
    let cols = r.varint("column count")? as usize;
    if cols == 0 || cols > 256 {
        return Err(Error::LengthMismatch {
            what: "column count",
        });
    }
    let mut out = Vec::with_capacity(cols);
    for _ in 0..cols {
        let id = r.u32("column id")?;
        let name = r.len_slice("column name")?;
        let type_tag = r.u8("type tag")?;
        let encoding = r.varint("encoding tag")?;
        let dict = r.u8("dictionary flag")?;
        let chunk_version = if version == VERSION_V2 {
            r.u16("chunk version")?
        } else {
            VERSION_V1
        };
        let name = String::from_utf8_lossy(name).into_owned();
        out.push(ColumnDesc::new(
            id,
            name,
            type_tag,
            encoding,
            dict != 0,
            chunk_version,
        )?);
    }
    Schema::from_columns(out, version)
}

fn parse_rowgroups(body: &[u8]) -> Result<Vec<RowGroup>> {
    let mut r = Reader::new(body);
    let count = r.varint("row group count")? as usize;
    if count == 0 || count > 4096 {
        return Err(Error::LengthMismatch {
            what: "row group count",
        });
    }
    let mut groups = Vec::with_capacity(count);
    for _ in 0..count {
        let footer_rows = r.u32("row group rows")?;
        let pages = r.varint("page count")? as usize;
        if pages == 0 || pages > 4096 {
            return Err(Error::LengthMismatch { what: "page count" });
        }
        let mut refs = Vec::with_capacity(pages);
        for _ in 0..pages {
            refs.push(PageRef {
                offset: r.u32("page offset")?,
                len: r.u32("page length")?,
                rows: r.u32("page rows")?,
                encoding: r.varint("page encoding")?,
            });
        }
        groups.push(RowGroup {
            footer_rows,
            pages: refs,
        });
    }
    Ok(groups)
}

fn parse_footer(body: &[u8]) -> Result<Vec<ZoneMap>> {
    let declared_len = body.len();
    if declared_len < 4 {
        return Err(Error::UnexpectedEof { what: "footer" });
    }
    let (payload, crc_bytes) = body.split_at(declared_len - 4);
    let want = u32::from_le_bytes([crc_bytes[0], crc_bytes[1], crc_bytes[2], crc_bytes[3]]);
    if crc32c(payload) != want {
        return Err(Error::Checksum { what: "footer" });
    }
    let mut r2 = Reader::new(payload);
    let count = r2.varint("zone count")? as usize;
    let mut zones = Vec::with_capacity(count);
    for _ in 0..count {
        zones.push(ZoneMap {
            min: r2.i64("zone min")?,
            max: r2.i64("zone max")?,
            rows: r2.u32("zone rows")?,
        });
    }
    Ok(zones)
}

/// One staged row group: the footer row assertion plus its pages as
/// (page bytes, physical rows, encoding).
type GroupPages = Vec<(Vec<u8>, u32, u32)>;

/// Incremental writer for `.loam` images. Used by the CLI-less tooling,
/// tests, and the corpus builders.
#[derive(Debug, Default)]
pub struct FileWriter {
    version: u16,
    columns: Vec<ColumnDesc>,
    groups: Vec<(u32, GroupPages)>,
    zones: Vec<ZoneMap>,
}

impl FileWriter {
    pub fn new(version: u16) -> Self {
        FileWriter {
            version,
            ..Default::default()
        }
    }

    pub fn column(&mut self, id: u32, name: &str, type_tag: u8, encoding: u32, dict: bool) {
        let desc = ColumnDesc::new(id, name.to_string(), type_tag, encoding, dict, self.version)
            .expect("writer columns are valid");
        self.columns.push(desc);
    }

    /// Add a legacy (V1) column chunk to a V2 file.
    pub fn column_legacy(&mut self, id: u32, name: &str, type_tag: u8, encoding: u32, dict: bool) {
        let desc = ColumnDesc::new(id, name.to_string(), type_tag, encoding, dict, VERSION_V1)
            .expect("writer columns are valid");
        self.columns.push(desc);
    }

    /// Add a row group. `footer_rows` is what the footer will assert; callers
    /// writing a partial tail pass a count ahead of the pages.
    pub fn row_group(&mut self, footer_rows: u32, pages: Vec<(Vec<u8>, u32, u32)>, zone: ZoneMap) {
        self.groups.push((footer_rows, pages));
        self.zones.push(zone);
    }

    pub fn finish(self) -> Vec<u8> {
        // Serialize schema.
        let mut schema = Vec::new();
        write_varint(&mut schema, self.columns.len() as u32);
        for c in &self.columns {
            schema.extend_from_slice(&c.id.to_le_bytes());
            write_len_slice(&mut schema, c.name.as_bytes());
            schema.push(c.type_tag);
            write_varint(&mut schema, c.encoding);
            schema.push(u8::from(c.dictionary));
            if self.version == VERSION_V2 {
                schema.extend_from_slice(&c.chunk_version.to_le_bytes());
            }
        }

        // Pages are appended after the sections; compute offsets once the
        // directory and section sizes are known.
        let mut page_blob: Vec<u8> = Vec::new();
        let dir_size = 8 + 3 * 9;
        let schema_len = schema.len();
        let groups_start = dir_size + schema_len;

        // Pre-measure the row group section with placeholder offsets, then
        // re-serialize with real ones.
        let mut groups_len = 0usize;
        for (_rows, pages) in &self.groups {
            groups_len += 4 + 1 + pages.len() * 13;
        }
        let pages_start_estimate = groups_start + groups_len;
        let mut offset = pages_start_estimate as u32;
        let mut per_group_refs: Vec<Vec<PageRef>> = Vec::new();
        for (_rows, pages) in &self.groups {
            let mut refs = Vec::new();
            for (bytes, prows, enc) in pages {
                refs.push(PageRef {
                    offset,
                    len: bytes.len() as u32,
                    rows: *prows,
                    encoding: *enc,
                });
                offset += bytes.len() as u32;
                page_blob.extend_from_slice(bytes);
            }
            per_group_refs.push(refs);
        }

        let serialize_groups = |refs: &[Vec<PageRef>]| {
            let mut out = Vec::new();
            write_varint(&mut out, self.groups.len() as u32);
            for (i, (rows, _pages)) in self.groups.iter().enumerate() {
                out.extend_from_slice(&rows.to_le_bytes());
                write_varint(&mut out, refs[i].len() as u32);
                for r in &refs[i] {
                    out.extend_from_slice(&r.offset.to_le_bytes());
                    out.extend_from_slice(&r.len.to_le_bytes());
                    out.extend_from_slice(&r.rows.to_le_bytes());
                    write_varint(&mut out, r.encoding);
                }
            }
            out
        };

        let mut groups_ser = serialize_groups(&per_group_refs);
        let real_pages_start = groups_start + groups_ser.len();
        let shift = real_pages_start as i64 - pages_start_estimate as i64;
        if shift != 0 {
            for refs in &mut per_group_refs {
                for r in refs.iter_mut() {
                    r.offset = (r.offset as i64 + shift) as u32;
                }
            }
            groups_ser = serialize_groups(&per_group_refs);
        }

        let mut footer_payload = Vec::new();
        write_varint(&mut footer_payload, self.zones.len() as u32);
        for z in &self.zones {
            footer_payload.extend_from_slice(&z.min.to_le_bytes());
            footer_payload.extend_from_slice(&z.max.to_le_bytes());
            footer_payload.extend_from_slice(&z.rows.to_le_bytes());
        }
        let crc = crc32c(&footer_payload);
        let mut footer = footer_payload.clone();
        footer.extend_from_slice(&crc.to_le_bytes());

        let footer_start = real_pages_start + page_blob.len();
        let mut out = Vec::with_capacity(footer_start + footer.len());
        out.extend_from_slice(FILE_MAGIC);
        out.extend_from_slice(&self.version.to_le_bytes());
        out.extend_from_slice(&3u16.to_le_bytes());
        out.push(SEC_SCHEMA);
        out.extend_from_slice(&(dir_size as u32).to_le_bytes());
        out.extend_from_slice(&(schema_len as u32).to_le_bytes());
        out.push(SEC_ROWGROUPS);
        out.extend_from_slice(&(groups_start as u32).to_le_bytes());
        out.extend_from_slice(&(groups_ser.len() as u32).to_le_bytes());
        out.push(SEC_FOOTER);
        out.extend_from_slice(&(footer_start as u32).to_le_bytes());
        out.extend_from_slice(&(footer.len() as u32).to_le_bytes());
        out.extend_from_slice(&schema);
        out.extend_from_slice(&groups_ser);
        out.extend_from_slice(&page_blob);
        out.extend_from_slice(&footer);
        debug_assert_eq!(out.len(), footer_start + footer.len());
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::tags;

    fn sample_page() -> Vec<u8> {
        let mut p = Vec::new();
        write_varint(&mut p, tags::ENC_PLAIN);
        write_varint(&mut p, 4);
        p.extend_from_slice(&[1, 2, 3, 4]);
        p
    }

    #[test]
    fn writer_round_trip() {
        let mut w = FileWriter::new(VERSION_V2);
        w.column(1, "temp", tags::TYPE_I64, tags::ENC_PLAIN, false);
        w.row_group(
            4,
            vec![(sample_page(), 4, tags::ENC_PLAIN)],
            ZoneMap {
                min: 1,
                max: 4,
                rows: 4,
            },
        );
        let bytes = w.finish();
        let img = open(&bytes).expect("open");
        assert_eq!(img.version, VERSION_V2);
        assert_eq!(img.schema.columns().len(), 1);
        assert_eq!(img.groups.len(), 1);
        assert_eq!(img.zones[0].rows, 4);
        assert_eq!(img.physical_rows(0), 4);
        assert!(img.page_bytes(&img.groups[0].pages[0]).is_ok());
    }

    #[test]
    fn rejects_bad_magic() {
        let bytes = b"NOPE........".to_vec();
        assert!(open(&bytes).is_err());
    }

    #[test]
    fn rejects_bad_footer_crc() {
        let mut w = FileWriter::new(VERSION_V1);
        w.column(1, "a", tags::TYPE_I64, tags::ENC_PLAIN, false);
        w.row_group(
            4,
            vec![(sample_page(), 4, tags::ENC_PLAIN)],
            ZoneMap {
                min: 0,
                max: 1,
                rows: 4,
            },
        );
        let mut bytes = w.finish();
        let n = bytes.len();
        bytes[n - 1] ^= 0xff;
        assert!(matches!(open(&bytes), Err(Error::Checksum { .. })));
    }

    #[test]
    fn partial_tail_is_accepted_at_open() {
        let mut w = FileWriter::new(VERSION_V2);
        w.column(1, "a", tags::TYPE_I64, tags::ENC_PLAIN, false);
        // Footer asserts 40 rows but the only page carries 4.
        w.row_group(
            40,
            vec![(sample_page(), 4, tags::ENC_PLAIN)],
            ZoneMap {
                min: 0,
                max: 1,
                rows: 40,
            },
        );
        let bytes = w.finish();
        let img = open(&bytes).expect("open");
        assert_eq!(img.groups[0].footer_rows, 40);
        assert_eq!(img.physical_rows(0), 4);
    }

    #[test]
    fn multi_group_writer_offsets_resolve() {
        let mut w = FileWriter::new(VERSION_V2);
        w.column(1, "a", tags::TYPE_I64, tags::ENC_PLAIN, false);
        for g in 0..4 {
            let page = crate::page::write_page(
                tags::ENC_PLAIN,
                2,
                0,
                &crate::page::plain::write_i64(&[g as i64, (g + 1) as i64]),
            );
            w.row_group(
                2,
                vec![(page, 2, tags::ENC_PLAIN)],
                ZoneMap {
                    min: g as i64,
                    max: (g + 1) as i64,
                    rows: 2,
                },
            );
        }
        let bytes = w.finish();
        let img = open(&bytes).unwrap();
        for (i, group) in img.groups.iter().enumerate() {
            let blob = img.page_bytes(&group.pages[0]).unwrap();
            assert!(!blob.is_empty(), "group {i}");
        }
    }
}
