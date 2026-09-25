//! The fast path for manifests an embedder hands over in memory
//! ([`ManifestFreshness::Content`]): nothing on disk says when such a
//! manifest changed, so each one is compared with the lockfile by content
//! instead of by `package.json` mtime.

use super::{
    super::{Decision, ManifestFreshness},
    FOO_MANIFEST, content_check_decision_for, setup_content_check_project,
};
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
/// when no `package.json` exists to stat — the mtime path would have had
/// nothing to prove freshness with.
#[test]
fn content_mode_accepts_an_in_memory_manifest_without_a_file_on_disk() {
    let (dir, config) = setup_content_check_project();
    fs::remove_file(dir.path().join("package.json")).unwrap();
    let manifest = in_memory_manifest(&dir, FOO_MANIFEST);
    let project_manifests = [(dir.path().to_path_buf(), &manifest)];

    let by_mtime = content_check_decision_for(
        &dir,
        config,
        false,
        &project_manifests,
        ManifestFreshness::Mtime,
    );
    assert!(
        matches!(by_mtime, Decision::Skipped { reason } if reason.contains("stat")),
        "the mtime path cannot judge a manifest without a file, got {by_mtime:?}",
    );

    let by_content = content_check_decision_for(
        &dir,
        config,
        false,
        &project_manifests,
        ManifestFreshness::Content,
    );
    assert_eq!(by_content, Decision::UpToDate);
}

/// The in-memory manifest gains a dependency while the `package.json` on
/// disk stays exactly as the previous install validated it. By mtime the
/// project reads as unchanged; by content the lockfile no longer
/// satisfies it, so the install must run.
#[test]
fn content_mode_detects_a_dependency_added_to_an_in_memory_manifest() {
    let (dir, config) = setup_content_check_project();
    let manifest = in_memory_manifest(
        &dir,
        r#"{"name":"root","version":"1.0.0","dependencies":{"foo":"^1.0.0","bar":"^1.0.0"}}"#,
    );
    let project_manifests = [(dir.path().to_path_buf(), &manifest)];

    assert_eq!(
        content_check_decision_for(
            &dir,
            config,
            false,
            &project_manifests,
            ManifestFreshness::Mtime
        ),
        Decision::UpToDate,
        "the untouched package.json hides the in-memory change from the mtime path",
    );

    let by_content = content_check_decision_for(
        &dir,
        config,
        false,
        &project_manifests,
        ManifestFreshness::Content,
    );
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
    let manifest = in_memory_manifest(&dir, FOO_MANIFEST);

    let decision = content_check_decision_for(
        &dir,
        config,
        true,
        &[(dir.path().to_path_buf(), &manifest)],
        ManifestFreshness::Content,
    );
    assert_eq!(decision, Decision::UpToDate);

    let after = pnpm_workspace_state::load_workspace_state(dir.path())
        .unwrap()
        .unwrap()
        .last_validated_timestamp;
    assert!(after > before, "expected the state timestamp to advance ({before} -> {after})");
}
