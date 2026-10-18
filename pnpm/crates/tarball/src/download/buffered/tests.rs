use super::{ContentCoding, MAX_UNTRUSTED_PREALLOC_BYTES, decode_buffered_body};

#[test]
fn a_body_declared_larger_than_the_buffer_limit_is_not_decoded_here() {
    let tar = vec![0u8; 1024];
    let zstd = zstd::encode_all(tar.as_slice(), 3).unwrap();
    assert_eq!(decode_buffered_body(ContentCoding::Zstd, zstd.clone(), None), Ok(tar));
    let declared = Some(MAX_UNTRUSTED_PREALLOC_BYTES + 1);
    assert_eq!(decode_buffered_body(ContentCoding::Zstd, zstd.clone(), declared), Err(zstd));
}

#[test]
fn a_zstd_frame_that_declares_a_larger_size_is_not_decoded_here() {
    let large = vec![0u8; MAX_UNTRUSTED_PREALLOC_BYTES + 1];
    let mut encoder = zstd::stream::write::Encoder::new(Vec::new(), 1).unwrap();
    encoder.include_contentsize(true).unwrap();
    encoder
        .set_pledged_src_size(Some(large.len() as u64))
        .unwrap();
    std::io::Write::write_all(&mut encoder, &large).unwrap();
    let zstd = encoder.finish().unwrap();
    assert_eq!(decode_buffered_body(ContentCoding::Zstd, zstd.clone(), None), Err(zstd));
}
