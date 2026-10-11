use super::sync_file;
use std::io::Write as _;

#[test]
fn syncs_a_regular_file() {
    let mut file = tempfile::tempfile().unwrap();
    file.write_all(b"content").unwrap();
    sync_file(&file).unwrap();
}

#[cfg(target_vendor = "apple")]
#[test]
fn falls_back_only_when_full_fsync_is_unsupported() {
    use super::is_full_fsync_unsupported;
    use std::io::Error;
    for errno in [libc::ENOTTY, libc::ENOTSUP, libc::EOPNOTSUPP, libc::EINVAL] {
        assert!(is_full_fsync_unsupported(&Error::from_raw_os_error(errno)), "errno {errno}");
    }
    for errno in [libc::EIO, libc::ENOSPC, libc::EBADF] {
        assert!(!is_full_fsync_unsupported(&Error::from_raw_os_error(errno)), "errno {errno}");
    }
}
