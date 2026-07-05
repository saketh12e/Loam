//! Packed dictionaries, prefix pages, and bloom filters in flows.

use loam::dict::packed;
use loam::page::prefix;
use loam::stats::bloom::Bloom;

#[test]
fn packed_dict_flow() {
    let words: Vec<Vec<u8>> = ["app/a", "app/b", "app/c", "apple/1"]
        .iter()
        .map(|s| s.as_bytes().to_vec())
        .collect();
    let raw = packed::encode(&words).unwrap();
    assert_eq!(packed::decode(&raw).unwrap(), words);
    assert!(packed::estimated_saving(&words) > 0);
}

#[test]
fn prefix_page_grouped_flow() {
    let groups = vec![
        (
            b"data/2026/".to_vec(),
            vec![b"01/a".to_vec(), b"01/b".to_vec()],
        ),
        (Vec::new(), vec![b"02/a".to_vec()]),
    ];
    let raw = prefix::encode_groups(&groups).unwrap();
    let values = prefix::decode(&raw, 3).unwrap();
    assert_eq!(
        values,
        vec![
            b"data/2026/01/a".to_vec(),
            b"data/2026/01/b".to_vec(),
            b"data/2026/02/a".to_vec()
        ]
    );
}

#[test]
fn bloom_skips_absent_groups() {
    let bloom = Bloom::build(&[10, 20, 30]);
    assert!(bloom.check(20));
    let mut absent = 0;
    for v in [1, 2, 3, 40, 50] {
        if !bloom.check(v) {
            absent += 1;
        }
    }
    assert!(absent >= 3);
}
