use super::{LinkFailure, LinkFallback, LinkFallbackPolicy};
use pretty_assertions::assert_eq;
use std::io;

/// The codes the platforms' link calls return, each next to the failure it
/// is read as. A `PermissionDenied` without an OS code stands for the
/// errors a library wraps on the way up.
fn codes() -> Vec<(&'static str, io::Error, LinkFailure)> {
    let mut codes = vec![
        ("NotFound", io::Error::from(io::ErrorKind::NotFound), LinkFailure::Path),
        ("AlreadyExists", io::Error::from(io::ErrorKind::AlreadyExists), LinkFailure::Path),
        (
            "PermissionDenied without a code",
            io::Error::from(io::ErrorKind::PermissionDenied),
            LinkFailure::Denied,
        ),
        ("TooManyLinks", io::Error::from(io::ErrorKind::TooManyLinks), LinkFailure::TooManyLinks),
        ("Unsupported", io::Error::from(io::ErrorKind::Unsupported), LinkFailure::Unsupported),
        (
            "InvalidInput from reflink-copy",
            io::Error::from(io::ErrorKind::InvalidInput),
            LinkFailure::Unsupported,
        ),
        ("an unknown error", io::Error::other("unknown"), LinkFailure::Unsupported),
    ];
    #[cfg(unix)]
    codes.extend([
        ("EXDEV", io::Error::from_raw_os_error(libc::EXDEV), LinkFailure::CrossDevice),
        ("EPERM", io::Error::from_raw_os_error(libc::EPERM), LinkFailure::Denied),
        ("EACCES", io::Error::from_raw_os_error(libc::EACCES), LinkFailure::Denied),
        ("EMLINK", io::Error::from_raw_os_error(libc::EMLINK), LinkFailure::TooManyLinks),
        ("ENOTTY", io::Error::from_raw_os_error(libc::ENOTTY), LinkFailure::Unsupported),
        ("EOPNOTSUPP", io::Error::from_raw_os_error(libc::EOPNOTSUPP), LinkFailure::Unsupported),
        // `EEXIST` shares its number with Windows's `ERROR_NOT_SAME_DEVICE`.
        ("EEXIST", io::Error::from_raw_os_error(libc::EEXIST), LinkFailure::Path),
    ]);
    #[cfg(windows)]
    codes.extend([
        ("ERROR_NOT_SAME_DEVICE", io::Error::from_raw_os_error(17), LinkFailure::CrossDevice),
        ("ERROR_ACCESS_DENIED", io::Error::from_raw_os_error(5), LinkFailure::Denied),
        ("ERROR_TOO_MANY_LINKS", io::Error::from_raw_os_error(1142), LinkFailure::TooManyLinks),
        ("ERROR_INVALID_FUNCTION", io::Error::from_raw_os_error(1), LinkFailure::Unsupported),
        ("ERROR_SHARING_VIOLATION", io::Error::from_raw_os_error(32), LinkFailure::Unsupported),
    ]);
    codes
}

#[test]
fn every_code_is_read_as_one_failure() {
    for (name, error, failure) in codes() {
        assert_eq!(LinkFailure::of(&error), failure, "{name}: {error:?}");
    }
}

#[test]
fn a_sample_error_is_read_as_its_failure() {
    for failure in LinkFailure::ALL {
        assert_eq!(LinkFailure::of(&failure.sample_error()), failure);
    }
}

#[test]
fn every_failure_has_one_answer_under_each_policy() {
    let expected = [
        (LinkFailure::Path, LinkFallback::Fail, LinkFallback::Fail),
        (LinkFailure::CrossDevice, LinkFallback::NextMethod, LinkFallback::CopyFile),
        (LinkFailure::Denied, LinkFallback::NextMethod, LinkFallback::Fail),
        (LinkFailure::TooManyLinks, LinkFallback::CopyFile, LinkFallback::CopyFile),
        (LinkFailure::Unsupported, LinkFallback::NextMethod, LinkFallback::Fail),
    ];
    assert_eq!(expected.len(), LinkFailure::ALL.len());
    for (failure, next_method, copy_one_file) in expected {
        assert_eq!(failure.fallback(LinkFallbackPolicy::NextMethod), next_method, "{failure:?}");
        assert_eq!(failure.fallback(LinkFallbackPolicy::CopyOneFile), copy_one_file, "{failure:?}");
    }
}
