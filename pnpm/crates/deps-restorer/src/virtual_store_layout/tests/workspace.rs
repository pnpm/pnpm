use super::{super::VirtualStoreLayout, make_config, package_metadata, snapshot_with_link};
use pnpm_lockfile::{
    DirectoryResolution, LockfileResolution, PackageKey, SnapshotEntry, TarballResolution,
};
use pretty_assertions::{assert_eq, assert_ne};
use std::{
    collections::HashMap,
    ffi::OsStr,
    path::{Path, PathBuf},
};

/// Two unrelated projects that both depend on a `./dep` directory
/// resolve to the same snapshot key, name, and (absent) version — only
/// the lockfile directory tells their slots apart.
#[test]
fn directory_deps_get_a_slot_per_project() {
    let key: PackageKey = "dep@file:dep".parse().unwrap();
    let mut snapshots = HashMap::new();
    snapshots.insert(key.clone(), SnapshotEntry::default());
    let mut packages = HashMap::new();
    packages.insert(
        key.without_peer(),
        package_metadata(
            LockfileResolution::Directory(DirectoryResolution { directory: "dep".to_string() }),
            None,
        ),
    );

    let config = make_config(
        true,
        PathBuf::from("/tmp/proj/node_modules/.pnpm"),
        PathBuf::from("/tmp/store/links"),
    );
    let slot_in = |lockfile_dir: &str| {
        super::super::VirtualStoreLayout::new(
            &config,
            None,
            Some(&snapshots),
            Some(&packages),
            None,
            Some(Path::new(lockfile_dir)),
        )
        .slot_dir(&key)
    };

    let in_project_a = slot_in("/home/user/a");
    let in_project_b = slot_in("/home/user/b");
    assert_ne!(in_project_a, in_project_b);
    // The slot is `<root>/@/dep/<version>/<hash>`, so the version segment is
    // the hash directory's parent. Compared as a component rather than a
    // substring: the separator is native, `\` on Windows.
    assert_eq!(
        in_project_a.parent().and_then(Path::file_name),
        Some(OsStr::new("directory")),
        "directory deps take the anchored version segment; got {in_project_a:?}",
    );
}
/// The scope is the *resolved target*, not the project, so workspaces
/// that link the same directory keep sharing one slot — the case that
/// matters for a toolchain linked into many workspaces. Contrast
/// [`directory_deps_get_a_slot_per_project`], where the project itself
/// is the only thing that can tell two slots apart.
#[test]
fn link_deps_resolving_to_one_directory_share_a_slot_across_projects() {
    let key: PackageKey = "react-dom@18.3.1(react@shared)".parse().unwrap();
    let mut snapshots = HashMap::new();
    // Both projects sit one level under `/home/user`, so `../shared`
    // resolves to `/home/user/shared` from either.
    snapshots.insert(key.clone(), snapshot_with_link("react", "../shared"));
    let mut packages = HashMap::new();
    packages.insert(
        key.without_peer(),
        package_metadata(
            LockfileResolution::Tarball(TarballResolution {
                tarball: "https://registry.npmjs.org/react-dom/-/react-dom-18.3.1.tgz".to_string(),
                integrity: None,
                revision: None,
                git_hosted: None,
                path: None,
            }),
            Some("18.3.1"),
        ),
    );
    let config = make_config(
        true,
        PathBuf::from("/tmp/proj/node_modules/.pnpm"),
        PathBuf::from("/tmp/store/links"),
    );
    let slot_in = |lockfile_dir: &str| {
        VirtualStoreLayout::new(
            &config,
            None,
            Some(&snapshots),
            Some(&packages),
            None,
            Some(Path::new(lockfile_dir)),
        )
        .slot_dir(&key)
    };

    assert_eq!(slot_in("/home/user/a"), slot_in("/home/user/b"));
}
