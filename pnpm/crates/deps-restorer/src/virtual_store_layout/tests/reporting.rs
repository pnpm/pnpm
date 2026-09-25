use super::package_metadata;
use pnpm_lockfile::{LockfileResolution, PackageKey, TarballResolution};
use pretty_assertions::assert_eq;

/// A lockfile that keeps a `file:` snapshot but drops or mismatches its
/// `packages:` entry leaves no resolution to read. The segment and the
/// project scope have to reach the same verdict from what is left, or
/// the snapshot takes the anchored segment while missing the scope —
/// which is the collision the scope exists to prevent.
#[test]
fn a_file_snapshot_without_metadata_is_still_scoped() {
    let dir_dep: PackageKey = "b@file:packages/b".parse().unwrap();

    assert_eq!(super::super::gvs_version_segment(None, &dir_dep.suffix), "directory");
    assert_eq!(
        super::super::local_directory_scope(None, &dir_dep.suffix, Some("/home/user/a")),
        Some("/home/user/a"),
    );

    // The same holds when the entry is present but carries neither a
    // directory resolution nor a version to fall back on.
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
    assert_eq!(super::super::gvs_version_segment(Some(&bare), &dir_dep.suffix), "directory");
    assert_eq!(
        super::super::local_directory_scope(Some(&bare), &dir_dep.suffix, Some("/home/user/a")),
        Some("/home/user/a"),
    );
}
