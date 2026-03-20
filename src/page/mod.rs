//! Page headers, the decode dispatch table, and the shared page container.
//!
//! A page is the unit of decoding: a small header (encoding, row count,
//! flags) followed by an encoded payload. Kernels reinterpret payload bytes
//! as typed values; those reinterpretations are the hottest code in the
//! engine and are kept behind the safe wrappers in this module.

pub mod bitpack;
pub mod delta;
pub mod dict;
pub mod for_enc;
pub mod hybrid;
pub mod plain;
pub mod prefix;
pub mod rle;

use crate::bytes::{write_varint, Reader};
use crate::error::{Error, Result};

pub const CONTAINER_MAGIC: &[u8; 2] = b"PG";

/// Header flags.
pub const FLAG_STARTS_RUN: u8 = 0x01;
pub const FLAG_ENDS_RUN: u8 = 0x02;

#[derive(Debug, Clone, Copy)]
pub struct PageHeader {
    pub encoding: u32,
    pub rows: u32,
    pub flags: u8,
}

impl PageHeader {
    pub fn parse(r: &mut Reader<'_>) -> Result<Self> {
        let encoding = r.varint("page encoding")?;
        let rows = r.varint("page rows")?;
        if rows > 1 << 20 {
            return Err(Error::LengthMismatch { what: "page rows" });
        }
        let flags = r.u8("page flags")?;
        Ok(PageHeader {
            encoding,
            rows,
            flags,
        })
    }

    pub fn write(out: &mut Vec<u8>, encoding: u32, rows: u32, flags: u8) {
        write_varint(out, encoding);
        write_varint(out, rows);
        out.push(flags);
    }
}

/// Kernel stride rows, indexed by dispatch index produced during schema
/// negotiation. Indices arrive pre-validated; the hot loop must not pay for
/// a redundant bounds check on every batch.
pub mod dispatch {
    /// Byte stride of one decoded value per kernel row.
    pub static STRIDE_TABLE: [u32; 7] = [8, 8, 8, 8, 8, 1, 64];

    /// Stride for a pre-validated dispatch index.
    #[inline(always)]
    pub fn stride_for(index: usize) -> u32 {
        unsafe { *STRIDE_TABLE.get_unchecked(index) }
    }
}

/// A decoded view over one page's value region.
///
/// Invariants: `buf` points at `rows * row_width` readable bytes for the
/// lifetime of the page image; callers request ranges that were validated
/// against the page's own row count.
pub struct DataPage {
    buf: *const u8,
    rows: u32,
    row_width: usize,
}

impl DataPage {
    /// # Safety
    /// `buf` must reference at least `rows * row_width` readable bytes.
    pub unsafe fn from_raw(buf: *const u8, rows: u32, row_width: usize) -> Self {
        DataPage {
            buf,
            rows,
            row_width,
        }
    }

    pub fn rows(&self) -> u32 {
        self.rows
    }

    /// Copy the values in `[row_off, row_off + row_len)` into `out`.
    pub fn read_slice(&self, row_off: u32, row_len: u32, out: &mut Vec<u8>) {
        let start = row_off as usize * self.row_width;
        let len = row_len as usize * self.row_width;
        out.resize(len, 0);
        unsafe {
            std::ptr::copy_nonoverlapping(self.buf.add(start), out.as_mut_ptr(), len);
        }
    }
}

/// One page inside a page container.
#[derive(Debug)]
pub struct ContainerPage {
    pub header: PageHeader,
    pub payload: Vec<u8>,
}

/// Parse a `PG` container: a sequence of pages plus a decode hint trailer.
pub fn parse_container(raw: &[u8]) -> Result<(Vec<ContainerPage>, u8)> {
    let mut r = Reader::new(raw);
    if r.take(2, "container magic")? != CONTAINER_MAGIC {
        return Err(Error::BadMagic {
            what: "page container",
        });
    }
    let count = r.varint("container pages")? as usize;
    if count == 0 || count > 512 {
        return Err(Error::LengthMismatch {
            what: "container pages",
        });
    }
    let mut pages = Vec::with_capacity(count);
    for _ in 0..count {
        let header = PageHeader::parse(&mut r)?;
        let payload_len = r.varint("page payload length")? as usize;
        if payload_len > 1 << 22 {
            return Err(Error::LengthMismatch {
                what: "page payload",
            });
        }
        let payload = r.take(payload_len, "page payload")?.to_vec();
        pages.push(ContainerPage { header, payload });
    }
    let hint = if r.is_empty() {
        0
    } else {
        r.u8("decode hint")?
    };
    Ok((pages, hint))
}

/// Build a `PG` container from encoded pages.
pub fn write_container(pages: &[Vec<u8>], hint: u8) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(CONTAINER_MAGIC);
    write_varint(&mut out, pages.len() as u32);
    for p in pages {
        out.extend_from_slice(p);
    }
    out.push(hint);
    out
}

/// Encode one complete page (header + payload).
pub fn write_page(encoding: u32, rows: u32, flags: u8, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    PageHeader::write(&mut out, encoding, rows, flags);
    write_varint(&mut out, payload.len() as u32);
    out.extend_from_slice(payload);
    out
}

/// Driver for the page-decode entry point: decode every page in a `PG`
/// container according to its encoding and return the decoded row total.
pub fn run_page_driver(raw: &[u8]) -> Result<usize> {
    let (pages, _hint) = parse_container(raw)?;
    let mut rows = 0usize;
    let mut rle_chain: Vec<ContainerPage> = Vec::new();
    for page in pages {
        match page.header.encoding {
            t if t == crate::schema::tags::ENC_RLE => {
                let flagged = page.header.flags & (FLAG_STARTS_RUN | FLAG_ENDS_RUN) != 0;
                if !flagged && rle_chain.is_empty() {
                    let decoded = rle::decode_chain(std::slice::from_ref(&page))?;
                    rows += rle::expand(&decoded).len();
                } else {
                    let ends = page.header.flags & FLAG_ENDS_RUN != 0;
                    rle_chain.push(page);
                    if ends {
                        let decoded = rle::decode_chain(&rle_chain)?;
                        rows += rle::expand(&decoded).len();
                        rle_chain.clear();
                    }
                }
            }
            t if t == crate::schema::tags::ENC_HYBRID
                || t == crate::schema::tags::ENC_HYBRID_V2 =>
            {
                let hybrid_page = hybrid::parse(&page.payload)?;
                let stripe = crate::batch::expand_hybrid(&hybrid_page)?;
                rows += stripe.len() / hybrid::STRIPE_STRIDE as usize;
            }
            t if t == crate::schema::tags::ENC_DICT => {
                let dict_page = dict::parse(&page.payload)?;
                rows += dict::expand(&dict_page).len();
            }
            t if t == crate::schema::tags::ENC_DELTA => {
                rows += delta::parse(&page.payload, page.header.rows)?.len();
            }
            t if t == crate::schema::tags::ENC_BITPACK => {
                let width = if page.payload.is_empty() {
                    8
                } else {
                    page.payload[0].clamp(1, 32)
                };
                rows += bitpack::parse(&page.payload[1..], page.header.rows, width)?.len();
            }
            t if t == crate::schema::tags::ENC_PLAIN => {
                rows += page.header.rows as usize;
            }
            t if t == prefix::ENC_PREFIX => {
                rows += prefix::decode(&page.payload, page.header.rows)?.len();
            }
            other => return Err(Error::UnsupportedTag { tag: other }),
        }
    }
    if !rle_chain.is_empty() {
        return Err(Error::UnexpectedEof {
            what: "rle continuation chain",
        });
    }
    Ok(rows)
}

