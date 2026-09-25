use super::classify_from_ldd;
use crate::Implementation;

#[test]
fn glibc_detected_via_gnu_c_library() {
    assert_eq!(
        classify_from_ldd("#!/bin/bash\n# GNU C Library (glibc) ldd script\n"),
        Some(Implementation::Glibc),
    );
}

#[test]
fn glibc_detected_via_gnu_libc() {
    assert_eq!(classify_from_ldd("ldd (GNU libc) 2.39\n"), Some(Implementation::Glibc));
}

#[test]
fn musl_detected_via_ldd() {
    assert_eq!(classify_from_ldd("musl libc\nVersion 1.2.3\n"), Some(Implementation::Musl));
}

#[test]
fn unknown_ldd_content_returns_none() {
    assert_eq!(classify_from_ldd("not a libc at all\n"), None);
}

#[test]
fn empty_ldd_content_returns_none() {
    assert_eq!(classify_from_ldd(""), None);
}

#[test]
fn binary_content_does_not_block_detection() {
    let mut buf = vec![0u8; 2048];
    buf[..10].copy_from_slice(b"musl libc ");
    buf[10] = 0xFF;
    buf[11..18].copy_from_slice(b" 1.2.3\n");
    buf.truncate(18);
    let content = String::from_utf8_lossy(&buf);
    assert_eq!(classify_from_ldd(&content), Some(Implementation::Musl));
}
