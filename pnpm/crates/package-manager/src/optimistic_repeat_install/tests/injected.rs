//! Injected workspace dependencies: a project copied (not symlinked) into a
//! consumer's `node_modules` through a `file:` snapshot.

use super::{
    super::{Decision, ManifestFreshness},
    backdate_validated_files, content_check_decision_for, isolated_included, write_state,
};
use pnpm_config::Config;
use pnpm_lockfile::Lockfile;
use pnpm_package_manifest::PackageManifest;
use pnpm_workspace_state::ProjectEntry;
use std::{collections::BTreeMap, fs};
use tempfile::tempdir;

const LOCKFILE: &str = "lockfileVersion: '9.0'

settings:
  autoInstallPeers: true
  excludeLinksFromLockfile: false
  injectWorkspacePackages: true

importers:

  .:
    dependencies:
      pkg-a:
        specifier: workspace:*
        version: file:pkg-a

  pkg-a: {}

packages:

  pkg-a@file:pkg-a:
    resolution: {directory: pkg-a, type: directory}

snapshots:

  pkg-a@file:pkg-a: {}
";

/// The decision for a repeat install of a workspace whose root injects the
/// `pkg-a` project, with manifests handed over in memory: `pkg-a` has no
/// `package.json` on disk, and `sibling_manifest` is what it declares now.
fn injected_sibling_decision(
    shared_workspace_lockfile: bool,
    sibling_manifest: &serde_json::Value,
) -> Decision {
    let dir = tempdir().unwrap();
    let workspace_root = dir.path();
    let sibling_dir = workspace_root.join("pkg-a");
    fs::create_dir_all(&sibling_dir).unwrap();
    fs::write(workspace_root.join(Lockfile::FILE_NAME), LOCKFILE).unwrap();

    let mut config = Config::new();
    config.modules_dir = workspace_root.join("node_modules");
    config.install_state_dir = config.modules_dir.join(".pnpm");
    config.inject_workspace_packages = true;
    config.shared_workspace_lockfile = shared_workspace_lockfile;
    fs::create_dir_all(&config.modules_dir).unwrap();
    fs::create_dir_all(sibling_dir.join("node_modules")).unwrap();
    let config = config.leak();

    let settings = super::super::settings::current_settings(
        config,
        pnpm_config::NodeLinker::Isolated,
        isolated_included(),
        None,
    );
    let projects = BTreeMap::from([
        (
            workspace_root.to_string_lossy().into_owned(),
            ProjectEntry {
                name: Some("root".into()),
                version: Some("1.0.0".into()),
                has_modules_dir: true,
            },
        ),
        (
            sibling_dir.to_string_lossy().into_owned(),
            ProjectEntry {
                name: Some("pkg-a".into()),
                version: Some("1.0.0".into()),
                has_modules_dir: false,
            },
        ),
    ]);
    write_state(workspace_root, backdate_validated_files(workspace_root), settings, projects);

    let root_manifest = PackageManifest::from_value(
        workspace_root.join("package.json"),
        serde_json::json!({
            "name": "root",
            "version": "1.0.0",
            "dependencies": { "pkg-a": "workspace:*" },
        }),
    );
    let sibling_manifest =
        PackageManifest::from_value(sibling_dir.join("package.json"), sibling_manifest.clone());
    content_check_decision_for(
        &dir,
        config,
        true,
        &[(workspace_root.to_path_buf(), &root_manifest), (sibling_dir, &sibling_manifest)],
        ManifestFreshness::Content,
    )
}

fn unchanged_sibling() -> serde_json::Value {
    serde_json::json!({ "name": "pkg-a", "version": "1.0.0" })
}

/// With a shared lockfile the injected project is an importer of the same
/// lockfile, so the per-project manifest comparison already covers a change
/// to its dependencies and the fast path may stand.
#[test]
fn returns_up_to_date_for_an_injected_workspace_dependency_with_a_shared_lockfile() {
    assert_eq!(injected_sibling_decision(true, &unchanged_sibling()), Decision::UpToDate);
}

/// The injected project gained a dependency its `file:` snapshot does not
/// record, so the lockfile has to be re-resolved.
#[test]
fn returns_skipped_when_an_injected_workspace_dependency_gains_a_dependency() {
    let sibling = serde_json::json!({
        "name": "pkg-a",
        "version": "1.0.0",
        "dependencies": { "is-positive": "1.0.0" },
    });
    let decision = injected_sibling_decision(true, &sibling);
    assert!(
        matches!(decision, Decision::Skipped { reason } if reason.contains("no longer satisfied")),
        "expected Skipped(manifest no longer satisfied), got {decision:?}",
    );
}

/// With per-project lockfiles the consumer's lockfile records the injected
/// project's dependencies in its own `file:` snapshot, so the full install has
/// to re-check it.
#[test]
fn returns_skipped_for_an_injected_workspace_dependency_without_a_shared_lockfile() {
    let decision = injected_sibling_decision(false, &unchanged_sibling());
    assert!(
        matches!(decision, Decision::Skipped { reason } if reason.contains("local file dependency")),
        "expected Skipped(local file dependency), got {decision:?}",
    );
}
