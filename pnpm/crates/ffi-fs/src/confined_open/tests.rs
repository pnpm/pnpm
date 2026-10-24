use super::{open_components, open_directory_beneath};
use std::{fs, path::Path};

/// Pins that a descendant reached through no symlink opens, whichever
/// resolution the running platform takes.
#[test]
fn a_plain_descendant_opens() {
    let fixture = tempfile::tempdir().expect("a temporary directory");
    fs::create_dir_all(fixture.path().join("real/child")).expect("create the tree");
    let root = fs::File::open(fixture.path()).expect("open the fixture root");

    let (directory, opens) =
        open_directory_beneath(&root, Path::new("real/child")).expect("open the descendant");
    assert!(
        directory
            .metadata()
            .expect("stat the descendant")
            .is_dir(),
    );
    assert!((1..=2).contains(&opens), "unexpected open count: {opens}");
}

/// A symlink anywhere on the path is refused, which is the confinement the
/// call promises and the reason it exists rather than a plain `open`.
#[test]
fn a_symlinked_component_is_refused() {
    let fixture = tempfile::tempdir().expect("a temporary directory");
    fs::create_dir_all(fixture.path().join("real/child")).expect("create the tree");
    std::os::unix::fs::symlink("real", fixture.path().join("link")).expect("create the symlink");
    let root = fs::File::open(fixture.path()).expect("open the fixture root");

    let result = open_directory_beneath(&root, Path::new("link/child"));
    assert!(result.is_err(), "{result:?}");
}

/// Escaping the root is rejected before any lookup, so confinement does not
/// rest on the kernel's resolve flags alone.
#[test]
fn a_path_that_leaves_the_root_is_rejected() {
    let fixture = tempfile::tempdir().expect("a temporary directory");
    let root = fs::File::open(fixture.path()).expect("open the fixture root");

    for path in ["..", "real/../..", "/etc", ""] {
        let error =
            open_directory_beneath(&root, Path::new(path)).expect_err("the path is rejected");
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput, "{path}: {error:?}");
    }
}

/// The component-at-a-time fallback refuses the same symlinks as the
/// whole-path resolution, so a platform without the latter is no weaker.
#[test]
fn the_fallback_refuses_a_symlinked_component() {
    let fixture = tempfile::tempdir().expect("a temporary directory");
    fs::create_dir_all(fixture.path().join("real/child")).expect("create the tree");
    std::os::unix::fs::symlink("real", fixture.path().join("link")).expect("create the symlink");
    let root = fs::File::open(fixture.path()).expect("open the fixture root");

    let (directory, opens) =
        open_components(&root, Path::new("real/child")).expect("open the descendant");
    assert!(
        directory
            .metadata()
            .expect("stat the descendant")
            .is_dir(),
    );
    assert_eq!(opens, 2);
    let result = open_components(&root, Path::new("link/child"));
    assert!(result.is_err(), "{result:?}");
}
