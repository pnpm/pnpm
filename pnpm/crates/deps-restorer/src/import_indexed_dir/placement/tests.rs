use super::{dir_fits_at, file_fits_at};
use std::fs;
use tempfile::tempdir;

#[test]
fn nothing_at_a_path_fits_either_kind() {
    let root = tempdir().unwrap();
    let absent = root.path().join("absent");

    assert!(dir_fits_at(&absent));
    assert!(file_fits_at(&absent));
}

#[test]
fn a_directory_fits_only_where_a_directory_belongs() {
    let root = tempdir().unwrap();
    let target = root.path().join("target");
    fs::create_dir(&target).unwrap();

    assert!(dir_fits_at(&target));
    assert!(!file_fits_at(&target));
}

#[test]
fn a_file_fits_only_where_a_file_belongs() {
    let root = tempdir().unwrap();
    let target = root.path().join("target");
    fs::write(&target, b"payload").unwrap();

    assert!(file_fits_at(&target));
    assert!(!dir_fits_at(&target));
}
