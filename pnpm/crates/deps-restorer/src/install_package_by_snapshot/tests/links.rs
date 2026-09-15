#[cfg(unix)]
use super::super::InstallPackageBySnapshotError;
#[cfg(unix)]
use crate::install_package_by_snapshot::fetch::fetch_directory_resolution;
#[cfg(unix)]
use pnpm_directory_fetcher::DirectoryFetcherError;
#[cfg(unix)]
use pnpm_lockfile::DirectoryResolution;

#[cfg(unix)]
#[test]
fn directory_resolution_rejects_symlink_escape() {
    use std::os::unix::fs::symlink;

    let tmp = tempfile::tempdir().expect("tempdir");
    let workspace = tmp.path().join("workspace");
    let package_dir = workspace.join("packages/dep");
    let outside = tmp.path().join("outside");
    std::fs::create_dir_all(&package_dir).expect("create package dir");
    std::fs::create_dir_all(&outside).expect("create outside dir");
    std::fs::write(outside.join("secret.txt"), b"secret").expect("write outside file");
    symlink(&outside, package_dir.join("outside")).expect("create outside symlink");

    let err = fetch_directory_resolution(
        &workspace,
        &DirectoryResolution { directory: "packages/dep".to_string() },
        false,
    )
    .expect_err("outside symlink should be rejected");

    assert!(
        matches!(
            err,
            InstallPackageBySnapshotError::DirectoryFetch(
                DirectoryFetcherError::PathOutsideDirectory { .. },
            ),
        ),
        "expected path_escape error, got {err:?}",
    );
}
