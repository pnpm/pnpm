use super::{cleanup_outdated_minimum_release_age_excludes, resolved_package_versions};
use pacquet_config::Config;
use pacquet_lockfile::Lockfile;
use pacquet_package_manifest::PackageManifest;
use std::path::Path;
use tempfile::tempdir;

/// A package resolved only from a non-semver source registers its name
/// with an empty version set, so the cleanup pass keeps its bare-name
/// `minimumReleaseAgeExclude` entry while still pruning versioned ones.
#[test]
fn registers_non_semver_packages_by_name_only() {
    let lockfile = Lockfile::parse(
        "lockfileVersion: '9.0'\n\
         snapshots:\n  \
         foo@https://codeload.github.com/owner/repo/tarball/deadbeef: {}\n  \
         bar@1.0.0: {}\n",
        Path::new("pnpm-lock.yaml"),
    )
    .expect("lockfile parses")
    .expect("lockfile is non-empty");

    let resolved = resolved_package_versions(&lockfile);

    assert_eq!(resolved.get("foo").map(std::collections::BTreeSet::len), Some(0));
    assert_eq!(resolved.get("bar").map(|versions| versions.contains("1.0.0")), Some(true));
}

/// A registry-qualified snapshot key (`<name>@<registryName>:<version>`)
/// registers the version after the prefix, so a versioned
/// `minimumReleaseAgeExclude` entry for it survives the cleanup pass.
#[test]
fn registers_the_version_of_a_registry_qualified_key() {
    let lockfile = Lockfile::parse(
        "lockfileVersion: '9.0'\nsnapshots:\n  foo@myregistry:1.0.0: {}\n",
        Path::new("pnpm-lock.yaml"),
    )
    .expect("lockfile parses")
    .expect("lockfile is non-empty");

    let resolved = resolved_package_versions(&lockfile);

    assert_eq!(resolved.get("foo").map(|versions| versions.contains("1.0.0")), Some(true));
}

/// With `sharedWorkspaceLockfile: false` the install anchors the wanted
/// lockfile at the active project, so the cleanup pass reads it from the
/// project dir — not the workspace dir — while still writing
/// `pnpm-workspace.yaml` at the workspace dir. Pruning `foo@1.0.0` here
/// proves the project's lockfile (`bar@2.0.0` only) was the data source:
/// the workspace dir holds no lockfile, so a workspace-anchored read
/// would no-op and leave the entry in place.
#[test]
fn reads_the_project_lockfile_when_the_workspace_lockfile_is_not_shared() {
    let tmp = tempdir().expect("temp dir");
    let workspace_dir = tmp.path();
    let project_dir = workspace_dir.join("project");
    std::fs::create_dir_all(&project_dir).expect("create project dir");
    std::fs::write(
        workspace_dir.join("pnpm-workspace.yaml"),
        "minimumReleaseAgeExclude:\n  - foo@1.0.0\n",
    )
    .expect("write pnpm-workspace.yaml");
    std::fs::write(
        project_dir.join("pnpm-lock.yaml"),
        "lockfileVersion: '9.0'\nsnapshots:\n  bar@2.0.0: {}\n",
    )
    .expect("write project pnpm-lock.yaml");
    let manifest = PackageManifest::from_value(
        project_dir.join("package.json"),
        serde_json::json!({ "name": "project", "version": "1.0.0" }),
    );
    let mut config = Config::new();
    config.cleanup_outdated_minimum_release_age_excludes = true;
    config.shared_workspace_lockfile = false;

    cleanup_outdated_minimum_release_age_excludes(&config, Some(workspace_dir), &manifest)
        .expect("cleanup runs");

    assert!(
        !workspace_dir.join("pnpm-workspace.yaml").exists(),
        "the pruned-to-empty manifest must be deleted",
    );
}
