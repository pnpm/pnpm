use super::prepend_dirs_to_path;
use std::path::PathBuf;

#[test]
fn a_delimiter_in_a_directory_is_rejected() {
    let delimiter = if cfg!(windows) { "a;b" } else { "a:b" };
    let error =
        prepend_dirs_to_path(&[PathBuf::from(delimiter)]).expect_err("must reject the delimiter");
    assert_eq!(error.dir, delimiter);
}

#[test]
fn the_directories_come_first_in_the_order_given() {
    let (first, second) =
        if cfg!(windows) { (r"C:\store\bin", r"C:\node\bin") } else { ("/store/bin", "/node/bin") };
    let separator = if cfg!(windows) { ";" } else { ":" };
    let path = prepend_dirs_to_path(&[PathBuf::from(first), PathBuf::from(second)])
        .expect("normal dirs are accepted");
    let path = path.to_string_lossy().into_owned();

    assert!(path.starts_with(&format!("{first}{separator}{second}")), "{path}");
    let inherited = std::env::var("PATH").unwrap_or_default();
    if !inherited.is_empty() {
        assert!(path.ends_with(&inherited), "{path}");
    }
}
