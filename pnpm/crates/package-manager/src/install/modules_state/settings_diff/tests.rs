use super::same_path;
use std::path::Path;

#[test]
fn same_path_compares_lexically() {
    assert!(same_path(Path::new("/a/store/../store/v11"), Path::new("/a/store/v11")));
    assert!(!same_path(Path::new("/a/store/v11"), Path::new("/a/other/v11")));
}

#[cfg(windows)]
#[test]
fn same_path_ignores_non_ascii_case_on_windows() {
    assert!(same_path(Path::new(r"C:\Störe\v11"), Path::new(r"c:\STÖRE\v11")));
    assert!(!same_path(Path::new(r"C:\Störe\v11"), Path::new(r"C:\Store\v11")));
}
