use super::read_regular_file_capped;
use std::fs;

#[test]
fn reads_a_regular_file_under_the_cap() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("f");
    fs::write(&path, b"hello").unwrap();
    assert_eq!(read_regular_file_capped(&path, 64).unwrap().unwrap(), b"hello");
}

#[test]
fn absent_reads_as_none_and_oversized_as_an_error() {
    let temp = tempfile::tempdir().unwrap();
    assert!(read_regular_file_capped(&temp.path().join("missing"), 64).unwrap().is_none());
    let path = temp.path().join("big");
    fs::write(&path, vec![0u8; 65]).unwrap();
    assert!(read_regular_file_capped(&path, 64).is_err());
}

#[test]
#[cfg(unix)]
fn a_symlink_is_refused_at_open() {
    let temp = tempfile::tempdir().unwrap();
    let target = temp.path().join("target");
    fs::write(&target, b"x").unwrap();
    let link = temp.path().join("link");
    std::os::unix::fs::symlink(&target, &link).unwrap();
    assert!(read_regular_file_capped(&link, 64).is_err(), "symlinks must not be followed");
}

#[test]
#[cfg(target_os = "linux")]
fn a_fifo_is_refused_without_blocking() {
    let temp = tempfile::tempdir().unwrap();
    let fifo = temp.path().join("fifo");
    let c_path = std::ffi::CString::new(fifo.to_str().unwrap()).unwrap();
    assert_eq!(unsafe { libc_mkfifo(c_path.as_ptr()) }, 0, "mkfifo failed");
    // The whole point: this returns instead of hanging on the open.
    assert!(read_regular_file_capped(&fifo, 64).is_err());
}

#[cfg(target_os = "linux")]
unsafe fn libc_mkfifo(path: *const std::ffi::c_char) -> i32 {
    unsafe extern "C" {
        fn mkfifo(path: *const std::ffi::c_char, mode: u32) -> i32;
    }
    unsafe { mkfifo(path, 0o644) }
}
