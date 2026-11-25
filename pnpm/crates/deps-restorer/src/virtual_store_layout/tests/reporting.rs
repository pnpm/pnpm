use super::{
    make_config,
    package_metadata,
};
use pnpm_lockfile::{
    LockfileResolution,
    PackageKey,
    SnapshotEntry,
    TarballResolution,
};
use pretty_assertions::{
    assert_eq,
    assert_ne,
};
use std::{
    collections::HashMap,
    ffi::OsStr,
    path::{
        Path,
        PathBuf,
    },
};

/// A lockfile that keeps a `file:` snapshot but drops or mismatches its
/// `packages:` entry leaves no resolution to read. The segment and the
/// project scope have to reach the same verdict from what is left, or
/// the snapshot takes the anchored segment while missing the scope,
/// which is the collision the scope exists to prevent.
#[test]
fn a_file_snapshot_without_metadata_is_still_scoped() {
    let dir_dep: PackageKey = "b@file:packages/b".parse().unwrap();
    // Present, but with neither a directory resolution nor a version to
    // fall back on.
    let bare = package_metadata(
        LockfileResolution::Tarball(TarballResolution {
            tarball: "file:packages/b".to_string(),
            integrity: None,
            revision: None,
            git_hosted: None,
            path: None,
        }),
        None,
    );
    let snapshots = HashMap::from([(dir_dep.clone(), SnapshotEntry::default())]);
    let config = make_config(
        true,
        PathBuf::from("/tmp/proj/node_modules/.pnpm"),
        PathBuf::from("/tmp/store/links"),
    );

    for packages in [None, Some(HashMap::from([(dir_dep.without_peer(), bare)]))] {
        let slot_in = |lockfile_dir: &str| {
            super::super::VirtualStoreLayout::new(
                &config,
                None,
                Some(&snapshots),
                packages.as_ref(),
                None,
                Some(Path::new(lockfile_dir)),
            )
            .slot_dir(&dir_dep)
        };
        let slot = slot_in("/home/user/a");
        assert_ne!(slot, slot_in("/home/user/b"));
        assert_eq!(slot.parent().and_then(Path::file_name), Some(OsStr::new("directory")));
    }
}
