use super::{
    post_install_prune, prune_against_project_lockfiles, record_resolved_package_versions,
};
use pnpm_config::Config;
use pnpm_lockfile::Lockfile;
use pnpm_package_manifest::PackageManifest;
use pnpm_workspace_manifest_writer::ResolvedPackageVersions;
use std::path::Path;
use tempfile::tempdir;

fn resolved_package_versions(lockfile: &Lockfile) -> ResolvedPackageVersions {
    let mut resolved = ResolvedPackageVersions::new();
    record_resolved_package_versions(lockfile, &mut resolved);
    resolved
}

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
    assert_eq!(
        resolved
            .get("bar")
            .map(|versions| versions.contains("1.0.0")),
        Some(true),
    );
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

    assert_eq!(
        resolved
            .get("foo")
            .map(|versions| versions.contains("1.0.0")),
        Some(true),
    );
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

/// Under per-project lockfiles each entry is checked against the union of
/// every project's lockfile, the workspace root's included, so only the
/// entry no lockfile records is pruned.
#[test]
fn prunes_against_the_union_of_the_project_lockfiles() {
    let tmp = tempdir().expect("temp dir");
    let workspace_dir = tmp.path();
    std::fs::write(
        workspace_dir.join("pnpm-workspace.yaml"),
        "packages:\n  - pkgs/*\n\
         minimumReleaseAgeExclude:\n  - foo@1.0.0\n  - bar@2.0.0\n  - baz@3.0.0\n  - qux@4.0.0\n",
    )
    .expect("write pnpm-workspace.yaml");
    std::fs::write(workspace_dir.join("package.json"), r#"{"name":"root"}"#)
        .expect("write root package.json");
    std::fs::write(
        workspace_dir.join("pnpm-lock.yaml"),
        "lockfileVersion: '9.0'\nsnapshots:\n  baz@3.0.0: {}\n",
    )
    .expect("write root pnpm-lock.yaml");
    for (name, snapshot) in [("a", "foo@1.0.0"), ("b", "bar@2.0.0")] {
        let project_dir = workspace_dir.join("pkgs").join(name);
        std::fs::create_dir_all(&project_dir).expect("create project dir");
        std::fs::write(project_dir.join("package.json"), format!(r#"{{"name":"{name}"}}"#))
            .expect("write project package.json");
        std::fs::write(
            project_dir.join("pnpm-lock.yaml"),
            format!("lockfileVersion: '9.0'\nsnapshots:\n  {snapshot}: {{}}\n"),
        )
        .expect("write project pnpm-lock.yaml");
    }
    let mut config = Config::new();
    config.minimum_release_age_exclude_prune = true;
    config.shared_workspace_lockfile = false;

    prune_against_project_lockfiles(&config, workspace_dir).expect("cleanup runs");

    assert_eq!(
        std::fs::read_to_string(workspace_dir.join("pnpm-workspace.yaml"))
            .expect("read pnpm-workspace.yaml"),
        "packages:\n  - pkgs/*\n\
         minimumReleaseAgeExclude:\n  - foo@1.0.0\n  - bar@2.0.0\n  - baz@3.0.0\n",
    );
}
