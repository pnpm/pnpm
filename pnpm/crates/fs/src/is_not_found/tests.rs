use std::io;

use super::is_not_found;

#[test]
fn not_found_kind_is_not_found() {
    assert!(is_not_found(&io::Error::from(io::ErrorKind::NotFound)));
}

#[test]
fn other_kinds_are_found() {
    assert!(!is_not_found(&io::Error::from(io::ErrorKind::PermissionDenied)));
}

#[cfg(windows)]
#[test]
fn invalid_name_is_not_found_on_windows() {
    let invalid = std::env::temp_dir().join("patch:got@npm%3A11.8.2#~").join("package.json");
    let err = std::fs::read_to_string(invalid).expect_err("a name holding `:` cannot exist");
    assert_eq!(err.raw_os_error(), Some(123), "{err:?}");
    assert!(is_not_found(&err));
}
