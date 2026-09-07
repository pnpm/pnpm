use super::{lexical_normalize, lexical_normalize_posix};
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

/// The output is rebuilt even when there is no dot component to
/// resolve: consumers hash and compare normalized paths, so trailing
/// and doubled separators must not survive.
#[test]
fn strips_redundant_separators() {
    assert_eq!(lexical_normalize(Path::new("foo/bar/")), Path::new("foo/bar"));
    assert_eq!(lexical_normalize(Path::new("foo//bar")), Path::new("foo/bar"));
    assert_eq!(lexical_normalize(Path::new("/foo//bar/")), Path::new("/foo/bar"));
}

/// A drive letter followed by a colon is a legal file name component
/// once it is past the start of the path.
#[test]
#[cfg_attr(not(windows), ignore = "Windows path semantics")]
fn keeps_a_drive_like_component_in_the_middle_of_the_path() {
    assert_eq!(
        lexical_normalize(Path::new(r"C:\workspace\root\C:tools\shell.cmd")),
        Path::new(r"C:\workspace\root\C:tools\shell.cmd"),
    );
    assert_eq!(
        lexical_normalize(Path::new(r"C:\workspace\root\.\C:tools\..\shell.cmd")),
        Path::new(r"C:\workspace\root\shell.cmd"),
    );
}

#[test]
#[cfg_attr(not(windows), ignore = "Windows path semantics")]
fn keeps_windows_prefixes() {
    assert_eq!(lexical_normalize(Path::new(r"C:\foo\..\bar")), Path::new(r"C:\bar"));
    assert_eq!(lexical_normalize(Path::new(r"C:foo\.\bar")), Path::new(r"C:foo\bar"));
    assert_eq!(
        lexical_normalize(Path::new(r"\\server\share\foo\..\bar")),
        Path::new(r"\\server\share\bar"),
    );
    assert_eq!(lexical_normalize(Path::new(r"\foo\..\bar")), Path::new(r"\bar"));
}

#[test]
fn posix_normalization_preserves_relative_roots_and_trailing_slashes() {
    for (path, expected) in [
        ("", "."),
        ("././", "./"),
        ("foo/../", "./"),
        ("foo/../../bar//", "../bar/"),
        ("../../foo/../bar", "../../bar"),
        ("///foo/../../bar//", "/bar/"),
        ("/../../", "/"),
    ] {
        assert_eq!(lexical_normalize_posix(path), expected, "path: {path}");
    }
}

#[test]
fn posix_normalization_preserves_backslashes_and_drive_like_components() {
    for (path, expected) in [
        (r"./packages/foo\[bar\]", r"packages/foo\[bar\]"),
        (r"foo\bar/../baz", "baz"),
        (r"C:\foo\..\bar", r"C:\foo\..\bar"),
        ("C:/../bar", "bar"),
        ("./.hidden/../visible", "visible"),
    ] {
        assert_eq!(lexical_normalize_posix(path), expected, "path: {path}");
    }
}
