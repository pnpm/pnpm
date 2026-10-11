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
fn falls_back_to_fsync_when_full_sync_is_unsupported() {
    use super::sync_with_fallback;
    use std::{cell::Cell, io::Error};
    for errno in [libc::ENOTTY, libc::ENOTSUP, libc::EOPNOTSUPP, libc::EINVAL] {
        let fell_back = Cell::new(false);
        sync_with_fallback(
            || Err(Error::from_raw_os_error(errno)),
            || {
                fell_back.set(true);
                Ok(())
            },
        )
        .unwrap();
        assert!(fell_back.get(), "errno {errno}");
        let fsync_error = sync_with_fallback(
            || Err(Error::from_raw_os_error(errno)),
            || Err(Error::from_raw_os_error(libc::EIO)),
        )
        .unwrap_err();
        assert_eq!(fsync_error.raw_os_error(), Some(libc::EIO), "errno {errno}");
    }
}

#[cfg(target_vendor = "apple")]
#[test]
fn keeps_other_full_sync_errors() {
    use super::sync_with_fallback;
    use std::io::Error;
    for errno in [libc::EIO, libc::ENOSPC, libc::EBADF] {
        let error = sync_with_fallback(
            || Err(Error::from_raw_os_error(errno)),
            || panic!("fell back to fsync on errno {errno}"),
        )
        .unwrap_err();
        assert_eq!(error.raw_os_error(), Some(errno));
    }
}
