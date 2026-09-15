use crate::cross_device::is_cross_device;
use std::io;

/// `is_cross_device` picks up EXDEV (raw 18) on every Unix we
/// support, and Unix `EEXIST` (raw 17) must never be classified as
/// cross-device — misclassifying a concurrent-create race as EXDEV
/// would fall back to a copy and overwrite the file the other
/// process just installed. On Windows raw 17 is
/// `ERROR_NOT_SAME_DEVICE`, which is a genuine cross-device signal.
#[test]
fn is_cross_device_distinguishes_unix_eexist_from_windows_not_same_device() {
    #[cfg(unix)]
    {
        let exdev = io::Error::from_raw_os_error(18);
        assert!(is_cross_device(&exdev), "raw 18 is EXDEV on every Unix");

        let eexist = io::Error::from_raw_os_error(17);
        assert!(
            !is_cross_device(&eexist),
            "Unix EEXIST (raw 17) is NOT cross-device — misclassifying would overwrite files",
        );
    }
    #[cfg(windows)]
    {
        let not_same_device = io::Error::from_raw_os_error(17);
        assert!(
            is_cross_device(&not_same_device),
            "Windows ERROR_NOT_SAME_DEVICE (raw 17) IS cross-device",
        );

        let not_exdev = io::Error::from_raw_os_error(18);
        assert!(
            !is_cross_device(&not_exdev),
            "raw 18 on Windows is not the cross-device code — must not be classified as EXDEV",
        );
    }
}

#[test]
fn errors_without_an_os_code_are_not_cross_device() {
    assert!(!is_cross_device(&io::Error::other("no raw code")));
}
