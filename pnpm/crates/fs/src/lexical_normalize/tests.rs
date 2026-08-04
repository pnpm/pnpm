use super::lexical_normalize;
use std::path::Path;

#[test]
fn collapses_parent_dir_segments() {
    assert_eq!(lexical_normalize(Path::new("foo/bar/../baz")), Path::new("foo/baz"));
}

#[test]
fn drops_parent_dir_at_root() {
    assert_eq!(lexical_normalize(Path::new("/..")), Path::new("/"));
    assert_eq!(lexical_normalize(Path::new("/../foo")), Path::new("/foo"));
}

#[test]
fn preserves_leading_parent_dir_when_unanchored() {
    assert_eq!(lexical_normalize(Path::new("../foo")), Path::new("../foo"));
    assert_eq!(lexical_normalize(Path::new("../../foo")), Path::new("../../foo"));
}

#[test]
fn drops_current_dir_segments() {
    assert_eq!(lexical_normalize(Path::new("foo/./bar")), Path::new("foo/bar"));
    assert_eq!(lexical_normalize(Path::new("./foo")), Path::new("foo"));
}

#[test]
fn collapses_unanchored_absolute_join() {
    let modules_dir = Path::new("/private/tmp/pkg/node_modules");
    let stored_relative = Path::new("../../../../Users/zoltan/Library/pnpm/store/v11/links");
    let joined = modules_dir.join(stored_relative);
    assert_eq!(lexical_normalize(&joined), Path::new("/Users/zoltan/Library/pnpm/store/v11/links"));
}

#[test]
fn empty_path_is_empty() {
    assert_eq!(lexical_normalize(Path::new("")), Path::new(""));
}
