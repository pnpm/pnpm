use super::prepend_dirs_to_path;
use pretty_assertions::assert_eq;
use std::{ffi::OsString, path::PathBuf};

fn prepend_one(dir: &str, current: Option<OsString>) -> OsString {
    prepend_dirs_to_path(&[PathBuf::from(dir)], current).expect("a normal dir is accepted")
}

#[test]
fn prepends_the_bin_dir() {
    let delimiter = if cfg!(windows) { ";" } else { ":" };
    let out = prepend_one("/vended/bin", Some(OsString::from("/usr/bin")));
    assert_eq!(out, OsString::from(format!("/vended/bin{delimiter}/usr/bin")));
}

#[test]
fn skips_a_dir_already_leading_path() {
    let delimiter = if cfg!(windows) { ";" } else { ":" };
    let already_leading = OsString::from(format!("/vended/bin{delimiter}/usr/bin"));
    let out = prepend_one("/vended/bin", Some(already_leading.clone()));
    assert_eq!(out, already_leading);
}

#[test]
fn handles_a_missing_or_empty_path() {
    assert_eq!(prepend_one("/vended/bin", None), OsString::from("/vended/bin"));
    assert_eq!(prepend_one("/vended/bin", Some(OsString::new())), OsString::from("/vended/bin"));
}

#[test]
fn joins_multiple_dirs_in_order() {
    let delimiter = if cfg!(windows) { ";" } else { ":" };
    let out = prepend_dirs_to_path(
        &[PathBuf::from("/a"), PathBuf::from("/b")],
        Some(OsString::from("/usr/bin")),
    )
    .expect("normal dirs are accepted");
    assert_eq!(out, OsString::from(format!("/a{delimiter}/b{delimiter}/usr/bin")));
}

#[test]
fn rejects_a_delimiter_in_a_dir() {
    let dir = if cfg!(windows) { "a;b" } else { "a:b" };
    let error =
        prepend_dirs_to_path(&[PathBuf::from(dir)], None).expect_err("must reject the delimiter");
    assert_eq!(error.dir, dir);
}
