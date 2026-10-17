//! The fast path for manifests an embedder hands over in memory: the check
//! judges a manifest by its content, so neither a missing `package.json` nor
//! an untouched one stands in for it.

use super::{super::Decision, FOO_MANIFEST, content_check_decision, setup_content_check_project};
use pnpm_package_manifest::PackageManifest;
use std::fs;

/// The manifest as an embedder supplies it: a JSON value anchored at the
/// project's `package.json` path, whether or not that file exists.
fn in_memory_manifest(dir: &tempfile::TempDir, json: &str) -> PackageManifest {
    PackageManifest::from_value(
        dir.path().join("package.json"),
        serde_json::from_str(json).expect("manifest json"),
    )
}

/// An in-memory manifest that the lockfile satisfies is up to date even
/// when no `package.json` exists on disk.
#[test]
fn content_mode_accepts_an_in_memory_manifest_without_a_file_on_disk() {
    let (dir, config) = setup_content_check_project();
    fs::remove_file(dir.path().join("package.json")).unwrap();
    let manifest = in_memory_manifest(&dir, FOO_MANIFEST);
    let project_manifests = [(dir.path().to_path_buf(), &manifest)];

    let by_content = content_check_decision(&dir, config, false, &project_manifests);
    assert_eq!(by_content, Decision::UpToDate);
}

/// The in-memory manifest gains a dependency while the `package.json` on
/// disk stays exactly as the previous install validated it. The lockfile no
/// longer satisfies it, so the install must run.
#[test]
fn content_mode_detects_a_dependency_added_to_an_in_memory_manifest() {
    let (dir, config) = setup_content_check_project();
    let manifest = in_memory_manifest(
        &dir,
        r#"{"name":"root","version":"1.0.0","dependencies":{"foo":"^1.0.0","bar":"^1.0.0"}}"#,
    );
    let project_manifests = [(dir.path().to_path_buf(), &manifest)];

    let by_content = content_check_decision(&dir, config, false, &project_manifests);
    assert!(
        matches!(by_content, Decision::Skipped { reason } if reason.contains("no longer satisfied")),
        "expected the content check to reject the manifest, got {by_content:?}",
    );
}

/// Workspace branch: a passing content check of in-memory manifests
/// refreshes `lastValidatedTimestamp` like one of on-disk manifests does,
/// so the lockfile-mtime probe of the next run has a current baseline.
#[test]
fn content_mode_refreshes_the_workspace_state_after_a_passing_check() {
    let (dir, config) = setup_content_check_project();
    let before = pnpm_workspace_state::load_workspace_state(dir.path())
        .unwrap()
        .unwrap()
        .last_validated_timestamp;
    let manifest = in_memory_manifest(&dir, super::FOO_MANIFEST_EDITED);

    let decision =
        content_check_decision(&dir, config, true, &[(dir.path().to_path_buf(), &manifest)]);
    assert_eq!(decision, Decision::UpToDate);

    let after = pnpm_workspace_state::load_workspace_state(dir.path())
        .unwrap()
        .unwrap()
        .last_validated_timestamp;
    assert!(after > before, "expected the state timestamp to advance ({before} -> {after})");
}
