use super::{post_install_prune, resolved_package_versions};
use pnpm_config::Config;
use pnpm_lockfile::Lockfile;
use pnpm_package_manifest::PackageManifest;
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
/// lockfile at the active project, so it records that project's
/// dependencies only — while `minimumReleaseAgeExclude` in
/// `pnpm-workspace.yaml` governs every project. `foo@1.0.0` may well be a
/// sibling project's dependency, so the pass must not prune it off one
/// project's lockfile.
#[test]
fn skips_the_pass_when_the_workspace_lockfile_is_not_shared() {
    let tmp = tempdir().expect("temp dir");
    let workspace_dir = tmp.path();
    let project_dir = workspace_dir.join("project");
    std::fs::create_dir_all(&project_dir).expect("create project dir");
    let workspace_yaml = workspace_dir.join("pnpm-workspace.yaml");
    std::fs::write(&workspace_yaml, "minimumReleaseAgeExclude:\n  - foo@1.0.0\n")
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
    config.minimum_release_age_exclude_prune = true;
    config.shared_workspace_lockfile = false;

    post_install_prune(&config, Some(workspace_dir), &manifest).expect("cleanup runs");

    assert_eq!(
        std::fs::read_to_string(&workspace_yaml).expect("read pnpm-workspace.yaml"),
        "minimumReleaseAgeExclude:\n  - foo@1.0.0\n",
    );
}

/// The shared lockfile covers every project, so an entry no snapshot
/// records is pruned — here down to an empty manifest, which is deleted.
#[test]
fn prunes_against_the_shared_workspace_lockfile() {
    let tmp = tempdir().expect("temp dir");
    let workspace_dir = tmp.path();
    std::fs::write(
        workspace_dir.join("pnpm-workspace.yaml"),
        "minimumReleaseAgeExclude:\n  - foo@1.0.0\n",
    )
    .expect("write pnpm-workspace.yaml");
    std::fs::write(
        workspace_dir.join("pnpm-lock.yaml"),
        "lockfileVersion: '9.0'\nsnapshots:\n  bar@2.0.0: {}\n",
    )
    .expect("write pnpm-lock.yaml");
    let manifest = PackageManifest::from_value(
        workspace_dir.join("package.json"),
        serde_json::json!({ "name": "project", "version": "1.0.0" }),
    );
    let mut config = Config::new();
    config.minimum_release_age_exclude_prune = true;

    post_install_prune(&config, Some(workspace_dir), &manifest).expect("cleanup runs");

    assert!(
        !workspace_dir.join("pnpm-workspace.yaml").exists(),
        "the pruned-to-empty manifest must be deleted",
    );
}
