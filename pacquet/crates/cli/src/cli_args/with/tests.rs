use super::{WithError, prepend_to_path};
use std::path::Path;

#[test]
fn prepend_to_path_rejects_a_delimiter_in_the_bin_dir() {
    let delimiter = if cfg!(windows) { "a;b" } else { "a:b" };
    let error = prepend_to_path(Path::new(delimiter)).expect_err("must reject the delimiter");
    assert!(matches!(error, WithError::BadPathDir { .. }));
}

#[test]
fn prepend_to_path_accepts_a_normal_bin_dir() {
    let dir = if cfg!(windows) { r"C:\store\bin" } else { "/store/bin" };
    let path = prepend_to_path(Path::new(dir)).expect("a normal dir is accepted");
    assert!(path.to_string_lossy().starts_with(dir));
}
