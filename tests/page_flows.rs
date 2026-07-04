//! Page kernel coverage through the decode driver: containers, headers,
//! every encoding, and malformed inputs.

use loam::page::{
    bitpack, delta, dict, for_enc, hybrid, plain, prefix, rle, write_container, write_page,
    PageHeader,
};
use loam::schema::tags;

fn drive(pages: &[Vec<u8>]) -> loam::Result<usize> {
    loam::page::run_page_driver(&write_container(pages, 0))
}

#[test]
fn driver_handles_plain() {
    assert_eq!(
        drive(&[write_page(
            tags::ENC_PLAIN,
            4,
            0,
            &plain::write_i64(&[1, 2, 3, 4])
        )])
        .unwrap(),
        4
    );
}

#[test]
fn driver_handles_delta() {
    assert_eq!(
        drive(&[write_page(tags::ENC_DELTA, 3, 0, &delta::write(&[5, 7, 6]))]).unwrap(),
        3
    );
}

#[test]
fn driver_handles_bitpack() {
    let mut payload = vec![5u8];
    payload.extend_from_slice(&bitpack::write(&[1, 2, 3, 4], 5));
    assert_eq!(
        drive(&[write_page(tags::ENC_BITPACK, 4, 0, &payload)]).unwrap(),
        4
    );
}

#[test]
fn driver_handles_dict() {
    let payload = dict::write(&[b"x", b"y"], &[0, 1, 0]);
    assert_eq!(
        drive(&[write_page(tags::ENC_DICT, 3, 0, &payload)]).unwrap(),
        3
    );
}

#[test]
fn driver_handles_rle_single_and_chain() {
    let single = write_page(
        tags::ENC_RLE,
        6,
        0,
        &rle::write_runs(&[rle::Run { value: 1, len: 6 }]),
    );
    assert_eq!(drive(&[single]).unwrap(), 6);

    let p1 = write_page(
        tags::ENC_RLE,
        4,
        loam::page::FLAG_STARTS_RUN,
        &rle::write_runs(&[rle::Run { value: 2, len: 3 }, rle::Run { value: 3, len: 1 }]),
    );
    let p2 = write_page(
        tags::ENC_RLE,
        3,
        loam::page::FLAG_ENDS_RUN,
        &rle::write_runs(&[rle::Run { value: 3, len: 3 }]),
    );
    assert_eq!(drive(&[p1, p2]).unwrap(), 7);
}

#[test]
fn driver_handles_hybrid() {
    let payload = hybrid::write(&[b"aa", b"bb"], &[(0, 2), (1, 2)]);
    assert_eq!(
        drive(&[write_page(tags::ENC_HYBRID, 4, 0, &payload)]).unwrap(),
        4
    );
}

#[test]
fn driver_handles_prefix_groups() {
    let payload =
        prefix::encode_groups(&[(b"pre/".to_vec(), vec![b"a".to_vec(), b"b".to_vec()])]).unwrap();
    assert_eq!(
        drive(&[write_page(prefix::ENC_PREFIX, 2, 0, &payload)]).unwrap(),
        2
    );
}

#[test]
fn driver_rejects_unknown_encoding() {
    assert!(drive(&[write_page(99, 1, 0, &[0u8; 8])]).is_err());
}

#[test]
fn for_kernel_round_trip() {
    let values = vec![500i64, 501, 505, 502];
    let payload = for_enc::write(&values);
    assert_eq!(for_enc::parse(&payload, 4).unwrap(), values);
}

#[test]
fn header_row_cap_enforced() {
    let mut raw = Vec::new();
    loam::bytes::write_varint(&mut raw, tags::ENC_PLAIN);
    loam::bytes::write_varint(&mut raw, (1 << 20) + 1);
    raw.push(0);
    let mut r = loam::bytes::Reader::new(&raw);
    assert!(PageHeader::parse(&mut r).is_err());
}

#[test]
fn container_rejects_huge_page_count() {
    let mut raw = b"PG".to_vec();
    loam::bytes::write_varint(&mut raw, 999);
    assert!(loam::page::parse_container(&raw).is_err());
}

#[test]
fn mixed_container_decodes_in_order() {
    let plain_page = write_page(tags::ENC_PLAIN, 2, 0, &plain::write_i64(&[9, 9]));
    let delta_page = write_page(tags::ENC_DELTA, 2, 0, &delta::write(&[1, 3]));
    assert_eq!(drive(&[plain_page, delta_page]).unwrap(), 4);
}

#[test]
fn deep_containers_of_containers() {
    let pages: Vec<Vec<u8>> = (0..32)
        .map(|i| write_page(tags::ENC_PLAIN, 4, 0, &plain::write_i64(&[i as i64; 4])))
        .collect();
    assert_eq!(drive(&pages).unwrap(), 128);
}

#[test]
fn rle_alternating_runs_expand() {
    let runs: Vec<rle::Run> = (0..60)
        .map(|i| rle::Run {
            value: (i % 2) as i64,
            len: 3,
        })
        .collect();
    let decoded = rle::expand(&runs);
    assert_eq!(decoded.len(), 180);
    assert_eq!(decoded[0], 0);
    assert_eq!(decoded[3], 1);
}

#[test]
fn hybrid_words_at_stride_boundary() {
    let w = vec![b'x'; 64];
    let payload = hybrid::write(&[&w], &[(0, 4)]);
    let page = hybrid::parse(&payload).unwrap();
    assert_eq!(hybrid::expanded_rows(&page).unwrap(), 4);
}

#[test]
fn for_frame_handles_negatives() {
    let values = vec![-500i64, -499, -450, -501];
    let decoded = for_enc::parse(&for_enc::write(&values), 4).unwrap();
    assert_eq!(decoded, values);
}

#[test]
fn bitpack_boundary_widths() {
    for width in [1u8, 7, 13, 31, 32] {
        let mask = if width == 32 {
            u64::from(u32::MAX)
        } else {
            (1u64 << width) - 1
        };
        let values: Vec<u64> = (0..50).map(|i| (i * 7 + 1) & mask).collect();
        let payload = bitpack::write(&values, width);
        assert_eq!(
            bitpack::parse(&payload, 50, width).unwrap(),
            values,
            "w{width}"
        );
    }
}

#[test]
fn delta_constant_series() {
    let values = vec![42i64; 100];
    let decoded = delta::parse(&delta::write(&values), 100).unwrap();
    assert_eq!(decoded, values);
}
