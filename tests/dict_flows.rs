//! Dictionary stack coverage: registry cycles, global domains, packed
//! tables, codec segments, and the builder.

use loam::dict::{codec, global::GlobalDicts, packed, DictRegistry, Dictionary};
use loam::page::dict as page_dict;

#[test]
fn registry_full_cycle() {
    let mut reg = DictRegistry::default();
    reg.install(Dictionary::new(1, 1, vec![b"a".to_vec()]));
    reg.refresh(1, vec![b"b".to_vec()], 1);
    reg.refresh(1, vec![b"c".to_vec()], 1);
    assert_eq!(reg.current(1).unwrap().generation, 3);
    assert_eq!(reg.live_generations(), 2);
    reg.release_retired(1);
    assert_eq!(reg.live_generations(), 1);
    reg.retire_pass();
    assert_eq!(reg.live_generations(), 1);
    reg.release_retired(2);
    assert_eq!(reg.live_generations(), 0);
}

#[test]
fn missing_column_errors() {
    let reg = DictRegistry::default();
    assert!(reg.current(42).is_err());
}

#[test]
fn global_domains_isolated() {
    let mut g = GlobalDicts::default();
    g.install("a", vec![b"x".to_vec()]);
    g.install("b", vec![b"x".to_vec(), b"y".to_vec()]);
    assert_eq!(g.lookup("a", b"x"), Some(0));
    assert_eq!(g.lookup("b", b"y"), Some(1));
    assert_eq!(g.lookup("a", b"y"), None);
}

#[test]
fn packed_table_large_prefixes() {
    let words: Vec<Vec<u8>> = (0..100)
        .map(|i| format!("common/prefix/{i:04}").into_bytes())
        .collect();
    let raw = packed::encode(&words).unwrap();
    assert_eq!(packed::decode(&raw).unwrap(), words);
    assert!(packed::estimated_saving(&words) > 500);
}

#[test]
fn codec_segment_flow() {
    let words = vec![b"us-east".to_vec(), b"us-west".to_vec()];
    let seg = codec::encode(&words);
    assert_eq!(codec::decode(&seg).unwrap(), words);
    assert!(codec::decode(b"ZZ\x01\x00").is_err());
}

#[test]
fn builder_density_drives_choice() {
    let many_same: Vec<&[u8]> = (0..100).map(|_| b"same".as_slice()).collect();
    let mut b = loam::dict::builder::DictBuilder::new();
    b.push_all(&many_same);
    assert!(b.density() < 0.02);
    let payload = b.into_payload().unwrap();
    let page = page_dict::parse(&payload).unwrap();
    assert_eq!(page.words.len(), 1);
}

#[test]
fn cursor_survives_same_generation_reads() {
    let store = loam::dict::WordStore::build(&[b"a".to_vec(), b"b".to_vec()]);
    let cursor = unsafe { loam::dict::DictCursor::bind(&store) };
    for id in [0u32, 1, 0, 1, 1] {
        let _ = cursor.word(id);
    }
    assert_eq!(cursor.words(), 2);
}

#[test]
fn packed_single_word_table() {
    let words = vec![b"only".to_vec()];
    let raw = packed::encode(&words).unwrap();
    assert_eq!(packed::decode(&raw).unwrap(), words);
}

#[test]
fn frozen_prefix_edge() {
    let d =
        loam::dict::frozen::FrozenDict::build(vec![b"a".to_vec(), b"aa".to_vec(), b"ab".to_vec()])
            .unwrap();
    assert_eq!(d.prefix_range(b"a"), (0, 3));
    assert_eq!(d.lookup(b"ab"), Some(2));
}
