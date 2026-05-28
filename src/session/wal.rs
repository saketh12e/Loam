//! Write-ahead log for session operations: append-only journal that lets a
//! crashed session replay staged work before acknowledging it.
//!
//! Journal frames are checksummed; torn tails are detected at recovery and
//! ignored from the first bad frame onward.

use crate::bytes::{crc32c, write_len_slice, Reader};
use crate::error::{Error, Result};

/// One journal frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub sequence: u64,
    pub payload: Vec<u8>,
}

/// Append a frame to the journal.
pub fn append(journal: &mut Vec<u8>, sequence: u64, payload: &[u8]) {
    let mut frame = Vec::new();
    frame.extend_from_slice(&sequence.to_le_bytes());
    write_len_slice(&mut frame, payload);
    let crc = crc32c(&frame);
    frame.extend_from_slice(&crc.to_le_bytes());
    journal.extend_from_slice(&frame);
}

/// Recover all well-formed frames in sequence order, stopping at the first
/// torn or corrupted frame.
pub fn recover(journal: &[u8]) -> Result<Vec<Frame>> {
    let mut r = Reader::new(journal);
    let mut out = Vec::new();
    let mut expect = 1u64;
    while !r.is_empty() {
        if r.remaining() < 12 {
            break; // torn tail
        }
        let sequence = r.u64("journal sequence")?;
        let declared = match r.varint("journal payload length") {
            Ok(n) => n as usize,
            Err(_) => break,
        };
        if r.remaining() < declared + 4 {
            break; // torn payload or crc
        }
        let payload = r.take(declared, "journal payload")?;
        let want = r.u32("journal crc")?;
        let mut body = Vec::new();
        body.extend_from_slice(&sequence.to_le_bytes());
        write_len_slice(&mut body, payload);
        if crc32c(&body) != want {
            break; // corrupted frame: stop here
        }
        if sequence != expect {
            return Err(Error::Session {
                detail: format!("journal gap at {sequence}, want {expect}"),
            });
        }
        out.push(Frame {
            sequence,
            payload: payload.to_vec(),
        });
        expect += 1;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn append_and_recover() {
        let mut j = Vec::new();
        append(&mut j, 1, b"op1");
        append(&mut j, 2, b"op2");
        let frames = recover(&j).unwrap();
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[1].payload, b"op2");
    }

    #[test]
    fn torn_tail_is_ignored() {
        let mut j = Vec::new();
        append(&mut j, 1, b"op1");
        let mut partial = Vec::new();
        append(&mut partial, 2, b"op2-longer");
        j.extend_from_slice(&partial[..partial.len() - 5]);
        let frames = recover(&j).unwrap();
        assert_eq!(frames.len(), 1);
    }

    #[test]
    fn gap_is_an_error() {
        let mut j = Vec::new();
        append(&mut j, 1, b"a");
        append(&mut j, 3, b"b");
        assert!(recover(&j).is_err());
    }
}
