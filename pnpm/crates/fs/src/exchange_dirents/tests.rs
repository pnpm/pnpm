use super::exchange_dirents;
use std::{fs, io};

#[test]
fn exchanges_two_directories_or_reports_unsupported() {
    let root = tempfile::tempdir().unwrap();
    let first = root.path().join("first");
    let second = root.path().join("second");
    fs::create_dir(&first).unwrap();
    fs::create_dir(&second).unwrap();
    fs::write(first.join("marker"), b"first").unwrap();
    fs::write(second.join("marker"), b"second").unwrap();

    match exchange_dirents(&first, &second) {
        Ok(()) => {
            assert_eq!(fs::read(first.join("marker")).unwrap(), b"second");
            assert_eq!(fs::read(second.join("marker")).unwrap(), b"first");
        }
        Err(error) => {
            assert_eq!(error.kind(), io::ErrorKind::Unsupported, "{error}");
            assert_eq!(fs::read(first.join("marker")).unwrap(), b"first");
        }
    }
}

#[test]
fn a_missing_side_is_not_found() {
    let root = tempfile::tempdir().unwrap();
    let present = root.path().join("present");
    fs::create_dir(&present).unwrap();

    let error = exchange_dirents(&present, &root.path().join("missing")).unwrap_err();
    assert!(
        matches!(error.kind(), io::ErrorKind::NotFound | io::ErrorKind::Unsupported),
        "{error}",
    );
}
