use super::{
    InvalidWorkspaceManifestError, ReadWorkspaceManifestError, WORKSPACE_MANIFEST_FILENAME,
    WorkspaceManifest, read_workspace_manifest,
};
use pretty_assertions::assert_eq;
use std::fs;
use tempfile::TempDir;

#[test]
fn missing_file_returns_none() {
    let tmp = TempDir::new().unwrap();
    let manifest = read_workspace_manifest(tmp.path()).unwrap();
    assert_eq!(manifest, None);
}

#[test]
fn empty_file_returns_default() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join(WORKSPACE_MANIFEST_FILENAME), "").unwrap();
    let manifest = read_workspace_manifest(tmp.path()).unwrap();
    assert_eq!(manifest, Some(WorkspaceManifest::default()));
}

#[test]
fn parses_packages_array() {
    let tmp = TempDir::new().unwrap();
    fs::write(
        tmp.path().join(WORKSPACE_MANIFEST_FILENAME),
        "packages:\n  - packages/*\n  - apps/*\n",
    )
    .unwrap();
    let manifest = read_workspace_manifest(tmp.path()).unwrap().unwrap();
    assert_eq!(manifest.packages, Some(vec!["packages/*".to_string(), "apps/*".to_string()]));
}

/// Settings-only manifests (no `packages:`) leave `packages` as
/// `None` so [`find_workspace_projects`] can apply the
/// `['.', '**']` defaults. Matches upstream's
/// `opts.patterns ?? defaults` rule, where the fallback fires for
/// omitted-only, not for an explicit empty array. Distinguishing
/// the two states is the whole point of the [`Option`] wrapper.
///
/// [`find_workspace_projects`]: crate::find_workspace_projects
#[test]
fn settings_only_manifest_leaves_packages_none() {
    let tmp = TempDir::new().unwrap();
    fs::write(
        tmp.path().join(WORKSPACE_MANIFEST_FILENAME),
        "storeDir: /tmp/store\nregistry: https://example.com/\n",
    )
    .unwrap();
    let manifest = read_workspace_manifest(tmp.path()).unwrap().unwrap();
    assert_eq!(manifest.packages, None);
}

/// An explicit `packages: []` survives as `Some(vec![])` and is
/// distinct from the omitted case. Downstream this means "enumerate
/// only the workspace root project," not "fall back to the recursive
/// `**` default."
#[test]
fn empty_packages_array_preserved_as_some_empty() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join(WORKSPACE_MANIFEST_FILENAME), "packages: []\n").unwrap();
    let manifest = read_workspace_manifest(tmp.path()).unwrap().unwrap();
    assert_eq!(manifest.packages, Some(Vec::<String>::new()));
}

#[test]
fn empty_package_entry_rejected() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join(WORKSPACE_MANIFEST_FILENAME), "packages:\n  - ''\n  - apps/*\n")
        .unwrap();
    let err = read_workspace_manifest(tmp.path()).unwrap_err();
    assert!(
        matches!(
            err,
            ReadWorkspaceManifestError::Invalid(InvalidWorkspaceManifestError::EmptyPackageEntry),
        ),
        "unexpected error: {err}",
    );
}
