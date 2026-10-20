use super::CanonicalPaths;
use std::{fs, path::Path};

#[test]
fn matches_std_canonicalize_for_non_existent() {
    let paths = CanonicalPaths::default();
    let path = Path::new("this/path/does/not/exist");
    assert_eq!(
        paths.canonicalize(path).map_err(|error| error.kind()),
        fs::canonicalize(path).map_err(|error| error.kind()),
    );
}

/// Every case resolves twice, so the cached answer is checked too. The
/// case-variant spelling exists only on a case-insensitive filesystem, where
/// `std::fs::canonicalize` on macOS answers with the spelling on disk.
#[cfg(unix)]
#[test]
fn matches_std_canonicalize() {
    let temp = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(temp.path()).unwrap();
    fs::create_dir_all(root.join("ws/packages/app")).unwrap();
    fs::create_dir_all(root.join("ws/packages/lib")).unwrap();
    fs::create_dir_all(root.join("elsewhere/lib")).unwrap();
    fs::write(root.join("ws/packages/lib/package.json"), "{}").unwrap();
    std::os::unix::fs::symlink(root.join("elsewhere/lib"), root.join("ws/packages/linked"))
        .unwrap();
    std::os::unix::fs::symlink(root.join("missing"), root.join("ws/packages/dangling")).unwrap();

    let cases = [
        "ws/packages/app/../lib",
        "ws/packages/app/../lib/package.json",
        "ws/packages/app/./../lib/",
        "ws/packages/linked",
        "ws/packages/linked/../lib",
        "ws/packages/dangling",
        "ws/packages/missing/..",
        "ws/packages/lib/package.json/..",
        "ws/packages/lib/package.json/",
        "ws/packages/lib/package.json/.",
        "ws/Packages/LIB/package.json",
    ];
    let paths = CanonicalPaths::default();
    for case in cases {
        let path = format!("{}/{case}", root.display());
        for _ in 0..2 {
            let expected = fs::canonicalize(&path).map_err(|error| error.kind());
            let actual = paths
                .canonicalize(Path::new(&path))
                .map_err(|error| error.kind());
            assert_eq!(actual, expected, "{case}");
        }
    }
    for path in ["/..", "/", "relative/path"] {
        let expected = fs::canonicalize(path).map_err(|error| error.kind());
        assert_eq!(
            paths
                .canonicalize(Path::new(path))
                .map_err(|error| error.kind()),
            expected,
            "{path}",
        );
    }
}

/// A path the cache has resolved is answered without reading the
/// filesystem again.
#[cfg(unix)]
#[test]
fn answers_a_resolved_path_from_the_cache() {
    let temp = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(temp.path()).unwrap();
    fs::create_dir_all(root.join("ws/packages/app")).unwrap();
    fs::create_dir_all(root.join("ws/packages/lib")).unwrap();
    fs::write(root.join("ws/packages/lib/package.json"), "{}").unwrap();
    let manifest = root.join("ws/packages/app/../lib/package.json");
    let canonical = root.join("ws/packages/lib/package.json");

    let paths = CanonicalPaths::default();
    assert_eq!(paths.canonicalize(&manifest).unwrap(), canonical);

    fs::remove_dir_all(root.join("ws/packages/lib")).unwrap();
    assert_eq!(
        fs::canonicalize(&manifest).map_err(|error| error.kind()),
        Err(std::io::ErrorKind::NotFound),
    );
    assert_eq!(paths.canonicalize(&manifest).unwrap(), canonical);
    assert_eq!(
        paths
            .canonicalize(&root.join("ws/packages/lib"))
            .unwrap(),
        root.join("ws/packages/lib"),
    );
}
