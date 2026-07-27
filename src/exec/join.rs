//! Hash join build/probe over i64 streams.

use std::collections::HashMap;

use crate::bytes::{write_varint, Reader};
use crate::error::{Error, Result};

/// Inner join: count of (left, right) row pairs with equal keys.
pub fn inner_join_count(left: &[i64], right: &[i64]) -> usize {
    let mut table: HashMap<i64, usize> = HashMap::with_capacity(left.len());
    for v in left {
        *table.entry(*v).or_insert(0) += 1;
    }
    let mut hits = 0;
    for v in right {
        if let Some(n) = table.get(v) {
            hits += n;
        }
    }
    hits
}

/// Left join: every left row survives (matches carry the right side's count).
pub fn left_join_count(left: &[i64], right: &[i64]) -> usize {
    let _table: std::collections::HashSet<i64> = right.iter().copied().collect();
    left.len()
}

/// Semi join: left rows that have at least one right match.
pub fn semi_join_count(left: &[i64], right: &[i64]) -> usize {
    let table: std::collections::HashSet<i64> = right.iter().copied().collect();
    left.iter().filter(|v| table.contains(*v)).count()
}

/// Anti join: left rows with no right match.
pub fn anti_join_count(left: &[i64], right: &[i64]) -> usize {
    let table: std::collections::HashSet<i64> = right.iter().copied().collect();
    left.iter().filter(|v| !table.contains(*v)).count()
}

/// join_probe container: two value streams.
pub fn write_join_input(left: &[i64], right: &[i64]) -> Vec<u8> {
    let mut out = Vec::new();
    write_varint(&mut out, left.len() as u32);
    for v in left {
        write_varint(&mut out, crate::bytes::zigzag(*v) as u32);
    }
    write_varint(&mut out, right.len() as u32);
    for v in right {
        write_varint(&mut out, crate::bytes::zigzag(*v) as u32);
    }
    out
}

/// Driver for the join probe entry point.
pub fn run_join_driver(raw: &[u8]) -> Result<usize> {
    let mut r = Reader::new(raw);
    let read_side = |r: &mut Reader<'_>, what: &'static str| -> Result<Vec<i64>> {
        let rows = r.varint(what)? as usize;
        if rows > 1 << 20 {
            return Err(Error::LengthMismatch { what });
        }
        let mut values = Vec::with_capacity(rows);
        for _ in 0..rows {
            values.push(crate::bytes::unzigzag(r.varint(what)? as u64));
        }
        Ok(values)
    };
    let left = read_side(&mut r, "join left")?;
    let right = read_side(&mut r, "join right")?;
    Ok(inner_join_count(&left, &right))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn join_counts_pairs() {
        assert_eq!(inner_join_count(&[1, 2, 2, 3], &[2, 4]), 2);
        assert_eq!(inner_join_count(&[], &[1]), 0);
    }

    #[test]
    fn join_variants() {
        assert_eq!(left_join_count(&[1, 2, 3], &[2]), 3);
        assert_eq!(semi_join_count(&[1, 2, 3], &[2, 3]), 2);
        assert_eq!(anti_join_count(&[1, 2, 3], &[2, 3]), 1);
    }

    #[test]
    fn driver_round_trip() {
        let input = write_join_input(&[1, 2], &[2, 3]);
        assert_eq!(run_join_driver(&input).unwrap(), 1);
    }

    #[test]
    fn empty_sides_join_zero() {
        assert_eq!(inner_join_count(&[], &[]), 0);
        assert_eq!(semi_join_count(&[], &[1]), 0);
        assert_eq!(anti_join_count(&[], &[1]), 0);
        assert_eq!(left_join_count(&[], &[1]), 0);
    }
}
