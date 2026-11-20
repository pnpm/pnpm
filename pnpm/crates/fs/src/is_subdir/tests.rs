use std::path::Path;

use super::is_subdir;

#[test]
fn child_inside_parent() {
    assert!(is_subdir(Path::new("/a/b"), Path::new("/a/b/c")));
}

#[test]
fn child_equal_to_parent() {
    assert!(is_subdir(Path::new("/a/b"), Path::new("/a/b")));
}

#[test]
fn child_outside_parent() {
    assert!(!is_subdir(Path::new("/a/b"), Path::new("/a/c")));
}

#[test]
fn collapses_dot_dot_segments_before_compare() {
    assert!(is_subdir(Path::new("/a/b"), Path::new("/a/b/c/../d")));
    assert!(!is_subdir(Path::new("/a/b"), Path::new("/a/b/../c")));
}
