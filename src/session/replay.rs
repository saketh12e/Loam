//! Session script inspection: count and summarize operations in a script
//! without executing it (used by tooling and support flows).

use crate::bytes::Reader;
use crate::error::Result;
use crate::session::ops;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ScriptSummary {
    pub ops: usize,
    pub ingests: usize,
    pub queries: usize,
    pub refreshes: usize,
    pub views: usize,
}

/// Walk a script and tally its operations. Structural only; payloads are
/// skipped, not validated.
pub fn summarize(raw: &[u8]) -> Result<ScriptSummary> {
    let mut r = Reader::new(raw);
    let mut out = ScriptSummary::default();
    while !r.is_empty() {
        let op = r.u8("script op")?;
        out.ops += 1;
        match op {
            ops::INGEST => {
                out.ingests += 1;
                r.varint("ingest page")?;
                let n = r.varint("ingest bytes")? as usize;
                r.skip(n, "ingest bytes")?;
            }
            ops::HOLD | ops::FORK | ops::READ | ops::INVALIDATE_VIEW => {
                r.varint("op arg")?;
            }
            ops::INSTALL_DICT | ops::REFRESH_DICT => {
                if op == ops::REFRESH_DICT {
                    out.refreshes += 1;
                }
                r.varint("dict column")?;
                if op == ops::REFRESH_DICT {
                    r.varint("refresh refs")?;
                }
                skip_words(&mut r)?;
            }
            ops::QUERY_DICT => {
                out.queries += 1;
                r.varint("query column")?;
                r.varint("query encoding")?;
                let count = r.varint("query ids")? as usize;
                for _ in 0..count {
                    r.varint("query id")?;
                }
            }
            ops::CREATE_VIEW => {
                out.views += 1;
                r.varint("view column")?;
                r.varint("view generation")?;
                let n = r.varint("view text")? as usize;
                r.skip(n, "view text")?;
            }
            ops::BEGIN_SWEEP => {}
            ops::CLOSE => return Ok(out),
            other => {
                return Err(crate::error::Error::Session {
                    detail: format!("unknown op {other:#x}"),
                })
            }
        }
    }
    Ok(out)
}

fn skip_words(r: &mut Reader<'_>) -> Result<()> {
    let count = r.varint("word count")? as usize;
    for _ in 0..count {
        let n = r.varint("dict word")? as usize;
        r.skip(n, "dict word")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::ScriptWriter;

    #[test]
    fn summarizes_a_script() {
        let script = ScriptWriter::new()
            .ingest(1, &[1u8; 8])
            .hold(1)
            .install_dict(1, &[b"a"])
            .query_dict(1, 3, &[0])
            .refresh_dict(1, 1, &[b"b"])
            .create_view(1, 1, "k > 0")
            .close()
            .finish();
        let s = summarize(&script).unwrap();
        assert_eq!(s.ingests, 1);
        assert_eq!(s.queries, 1);
        assert_eq!(s.refreshes, 1);
        assert_eq!(s.views, 1);
        assert!(s.ops >= 6);
    }

    #[test]
    fn rejects_unknown_op() {
        assert!(summarize(&[0xee]).is_err());
    }

    #[test]
    fn summarize_empty_script() {
        let s = summarize(&[]).unwrap();
        assert_eq!(s.ops, 0);
    }
}
