//! Entries that another process removes or renames while the walk reads them.

use super::super::{FsReadDir, Host, LISTING_ATTEMPTS, PackageRules, walk_files};
use std::{
    fs, io,
    path::Path,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
};

fn walked_files<Sys: FsReadDir>(pkg_dir: &Path) -> io::Result<Vec<String>> {
    let manifest = serde_json::json!({});
    let rules = PackageRules {
        workspace_dir: None,
        manifest: &manifest,
        include_alternate_manifests: true,
    };
    let mut files = Vec::new();
    walk_files::<Sys>(pkg_dir, rules, &mut |path, _| {
        let relative = path.strip_prefix(pkg_dir).unwrap();
        files.push(relative.to_string_lossy().replace('\\', "/"));
    })?;
    files.sort();
    Ok(files)
}

fn package_with_lib() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("package.json"), "{}").unwrap();
    fs::write(root.path().join("index.js"), "").unwrap();
    fs::create_dir(root.path().join("lib")).unwrap();
    fs::write(root.path().join("lib/a.js"), "").unwrap();
    root
}

fn not_found() -> io::Result<fs::DirEntry> {
    Err(io::Error::from(io::ErrorKind::NotFound))
}

#[test]
fn a_listing_failed_by_a_concurrent_rename_is_read_again() {
    struct InterruptedOnce;
    static INTERRUPTED: AtomicBool = AtomicBool::new(false);
    impl FsReadDir for InterruptedOnce {
        fn read_dir(path: &Path) -> io::Result<impl Iterator<Item = io::Result<fs::DirEntry>>> {
            let interrupt = !INTERRUPTED.swap(true, Ordering::SeqCst);
            let listed = if interrupt { 1 } else { usize::MAX };
            Ok(fs::read_dir(path)?
                .take(listed)
                .chain(interrupt.then(not_found)))
        }
    }
    let root = package_with_lib();

    let files = walked_files::<InterruptedOnce>(root.path()).unwrap();

    assert_eq!(files, ["index.js", "lib/a.js", "package.json"]);
}

#[test]
fn a_listing_that_keeps_failing_is_an_error() {
    struct AlwaysInterrupted;
    static ATTEMPTS: AtomicUsize = AtomicUsize::new(0);
    impl FsReadDir for AlwaysInterrupted {
        fn read_dir(path: &Path) -> io::Result<impl Iterator<Item = io::Result<fs::DirEntry>>> {
            ATTEMPTS.fetch_add(1, Ordering::SeqCst);
            Ok(fs::read_dir(path)?.chain([not_found()]))
        }
    }
    let root = package_with_lib();

    let error = walked_files::<AlwaysInterrupted>(root.path()).unwrap_err();

    assert_eq!(error.kind(), io::ErrorKind::NotFound);
    assert_eq!(ATTEMPTS.load(Ordering::SeqCst), LISTING_ATTEMPTS);
}

#[test]
fn a_directory_removed_before_the_walk_enters_it_is_skipped() {
    struct RemovesLibAfterListing;
    impl FsReadDir for RemovesLibAfterListing {
        fn read_dir(path: &Path) -> io::Result<impl Iterator<Item = io::Result<fs::DirEntry>>> {
            let entries: Vec<_> = fs::read_dir(path)?.collect();
            let lib = path.join("lib");
            if lib.is_dir() {
                fs::remove_dir_all(lib)?;
            }
            Ok(entries.into_iter())
        }
    }
    let root = package_with_lib();

    let files = walked_files::<RemovesLibAfterListing>(root.path()).unwrap();

    assert_eq!(files, ["index.js", "package.json"]);
}

#[test]
fn a_missing_package_root_is_an_error() {
    let base = tempfile::tempdir().unwrap();

    let error = walked_files::<Host>(&base.path().join("missing")).unwrap_err();

    assert_eq!(error.kind(), io::ErrorKind::NotFound);
}
