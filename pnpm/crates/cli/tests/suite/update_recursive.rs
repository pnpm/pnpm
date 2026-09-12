//! Recursive `pacquet update` integration tests, ported from
//! `pnpm11/installing/commands/test/update/recursive.ts`.
//!
//! The cases that drive `latest` around mid-test run against a registry
//! of their own — see [`setup_with_own_registry`] — because moving a
//! dist tag mutates the storage the registry serves.

use crate::_utils::{append_workspace_yaml_key, lockfile_package_keys};
use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_modules_yaml::{Host as ModulesHost, IncludedDependencies, read_modules_manifest};
use pnpm_package_manifest::{DependencyGroup, PackageManifest};
use pnpm_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use pretty_assertions::assert_eq;
use serde_json::{Value, json};
use std::{ffi::OsStr, fs, path::Path, process::Command};
use tempfile::TempDir;

/// Published at 100.0.0, 100.1.0, and 101.0.0.
const DEP: &str = "@pnpm.e2e/dep-of-pkg-with-1-dep";
const FOO: &str = "@pnpm.e2e/foo";
const BAR: &str = "@pnpm.e2e/bar";
const QAR: &str = "@pnpm.e2e/qar";
const PEER_C: &str = "@pnpm.e2e/peer-c";
const PRINT_VERSION: &str = "@pnpm.e2e/print-version";
/// Stands in for upstream's `@zkochan/async-regex-replace`: a second
/// package in the other project that the selectors also name. The
/// fixture registry has no copy of that package.
const MULTI_VERSION_B: &str = "@pnpm.e2e/multi-version-b";
/// Depends on `@pnpm.e2e/dep-of-pkg-with-1-dep@^100.0.0`.
const PKG_WITH_DEP: &str = "@pnpm.e2e/pkg-with-1-dep";

fn setup() -> (TempDir, std::path::PathBuf, AddMockedRegistry) {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    (root, workspace, npmrc_info)
}

/// [`setup`] over fixture storage this test owns, so it can move dist
/// tags mid-test the way the upstream tests' `addDistTag` does.
fn setup_with_own_registry() -> (TempDir, std::path::PathBuf, AddMockedRegistry) {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry_with_own_storage();
    (root, workspace, npmrc_info)
}

fn pacquet(workspace: &Path, args: impl IntoIterator<Item = impl AsRef<OsStr>>) -> Command {
    Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(workspace)
        .with_args(args)
}

/// Register `manifests` as workspace packages, each in its own
/// subdirectory. Appends to the `pnpm-workspace.yaml` the harness wrote,
/// which carries the mocked registry settings.
fn write_workspace(workspace: &Path, manifests: &[(&str, Value)]) {
    let yaml_path = workspace.join("pnpm-workspace.yaml");
    let mut yaml = fs::read_to_string(&yaml_path).expect("read pnpm-workspace.yaml");
    if !yaml.ends_with('\n') {
        yaml.push('\n');
    }
    let packages: Vec<String> = manifests.iter().map(|(name, _)| format!("  - '{name}'")).collect();
    fs::write(&yaml_path, format!("{yaml}packages:\n{}\n", packages.join("\n")))
        .expect("write pnpm-workspace.yaml");

    for (name, manifest) in manifests {
        let dir = workspace.join(name);
        fs::create_dir_all(&dir).expect("create project dir");
        fs::write(dir.join("package.json"), manifest.to_string()).expect("write package.json");
    }
}

fn dep_spec(project_dir: &Path, name: &str) -> Option<String> {
    let manifest = PackageManifest::from_path(project_dir.join("package.json")).unwrap();
    manifest
        .dependencies([DependencyGroup::Prod])
        .find(|(key, _)| *key == name)
        .map(|(_, spec)| spec.to_string())
}

fn has_module(project_dir: &Path, name: &str) -> bool {
    project_dir.join("node_modules").join(name).exists()
}

/// The names under a project's `node_modules`, with scope directories
/// expanded, for logging before existence assertions.
fn list_modules(project_dir: &Path) -> Vec<String> {
    fn names(dir: &Path) -> Vec<String> {
        let Ok(entries) = fs::read_dir(dir) else { return Vec::new() };
        entries
            .filter_map(Result::ok)
            .flat_map(|entry| {
                let name = entry.file_name().to_string_lossy().into_owned();
                if name.starts_with('@') {
                    names(&entry.path()).iter().map(|inner| format!("{name}/{inner}")).collect()
                } else {
                    vec![name]
                }
            })
            .collect()
    }
    names(&project_dir.join("node_modules"))
}

/// Where a project's `node_modules/<name>` resolves to, following the
/// symlink a workspace dependency is installed as.
fn module_target(project_dir: &Path, name: &str) -> Option<std::path::PathBuf> {
    dunce::canonicalize(project_dir.join("node_modules").join(name)).ok()
}

fn installed_version(project_dir: &Path, name: &str) -> Option<String> {
    let manifest_path = project_dir.join("node_modules").join(name).join("package.json");
    let contents = fs::read_to_string(manifest_path).ok()?;
    let value: Value = serde_json::from_str(&contents).ok()?;
    value["version"].as_str().map(str::to_string)
}

/// Ports `recursive update`: a versioned selector reaches every project
/// that already depends on the package, and no project gains it.
#[test]
fn recursive_update_only_reaches_projects_that_have_the_dependency() {
    let (root, workspace, anchor) = setup();

    write_workspace(
        &workspace,
        &[
            (
                "project-1",
                json!({ "name": "project-1", "version": "1.0.0",
                "dependencies": { DEP: "100.0.0" } }),
            ),
            (
                "project-2",
                json!({ "name": "project-2", "version": "1.0.0",
                "dependencies": { FOO: "1.0.0" } }),
            ),
        ],
    );
    pacquet(&workspace, ["install"]).assert().success();

    pacquet(&workspace, ["-r", "update", &format!("{DEP}@100.1.0")]).assert().success();

    assert_eq!(
        installed_version(&workspace.join("project-1"), DEP).as_deref(),
        Some("100.1.0"),
        "the project that declares it should have been updated",
    );
    let project_2 = workspace.join("project-2");
    eprintln!("project-2 node_modules: {:?}", list_modules(&project_2));
    assert!(!has_module(&project_2, DEP), "a project that never declared it must not gain it");

    drop((root, anchor));
}

/// Ports `recursive update in workspace should not add new dependencies`:
/// naming a package no project depends on fails at `--depth 0` and adds
/// it nowhere.
#[test]
fn recursive_update_does_not_add_a_dependency_no_project_declares() {
    let (root, workspace, anchor) = setup();

    write_workspace(
        &workspace,
        &[
            ("project-1", json!({ "name": "project-1", "version": "1.0.0" })),
            ("project-2", json!({ "name": "project-2", "version": "1.0.0" })),
        ],
    );
    pacquet(&workspace, ["install"]).assert().success();

    let output = pacquet(&workspace, ["-r", "update", "--depth", "0", DEP])
        .output()
        .expect("run pacquet update");

    let stderr = String::from_utf8_lossy(&output.stderr);
    eprintln!("STATUS: {}\nSTDERR:\n{stderr}", output.status);
    assert!(!output.status.success(), "updating an undeclared package should fail");
    assert!(
        stderr.contains("ERR_PNPM_NO_PACKAGE_IN_DEPENDENCIES"),
        "the failure must carry the NO_PACKAGE_IN_DEPENDENCIES code",
    );
    assert!(
        stderr.contains("None of the specified packages were found in the dependencies"),
        "the failure must carry the NO_PACKAGE_IN_DEPENDENCIES message",
    );
    for project in ["project-1", "project-2"] {
        let project_dir = workspace.join(project);
        eprintln!("{project} node_modules: {:?}", list_modules(&project_dir));
        assert!(!has_module(&project_dir, DEP), "{project} gained the dependency");
        assert_eq!(dep_spec(&project_dir, DEP), None, "{project}'s manifest gained the dependency");
    }

    drop((root, anchor));
}

/// Ports `recursive update with aliased workspace dependency (#7975)`: a
/// dependency aliased onto a workspace package keeps its specifier and
/// stays linked, rather than being rewritten or dropped.
#[test]
fn recursive_update_keeps_an_aliased_workspace_dependency() {
    let (root, workspace, anchor) = setup();

    write_workspace(
        &workspace,
        &[
            (
                "project-1",
                json!({ "name": "project-1", "version": "1.0.0",
                "dependencies": { "pkg": "workspace:project-2@^" } }),
            ),
            ("project-2", json!({ "name": "project-2", "version": "1.0.0" })),
        ],
    );
    pacquet(&workspace, ["install"]).assert().success();

    pacquet(&workspace, ["-r", "update", "--depth", "0"]).assert().success();

    let project_1 = workspace.join("project-1");
    eprintln!("project-1 node_modules: {:?}", list_modules(&project_1));
    assert_eq!(
        module_target(&project_1, "pkg"),
        dunce::canonicalize(workspace.join("project-2")).ok(),
        "the alias should stay linked to project-2",
    );
    assert_eq!(
        dep_spec(&project_1, "pkg").as_deref(),
        Some("workspace:project-2@^"),
        "the aliased workspace specifier should survive the update",
    );

    drop((root, anchor));
}

/// Ports `recursive update prod dependencies only`.
#[test]
fn recursive_update_prod_dependencies_only() {
    let (root, workspace, anchor) = setup_with_own_registry();
    anchor.set_dist_tag(FOO, "100.0.0", "latest");
    anchor.set_dist_tag(BAR, "100.0.0", "latest");

    write_workspace(
        &workspace,
        &[
            (
                "project-1",
                json!({ "name": "project-1", "version": "1.0.0",
                "dependencies": { FOO: "^100.0.0" } }),
            ),
            (
                "project-2",
                json!({ "name": "project-2", "version": "1.0.0",
                "devDependencies": { BAR: "^100.0.0" } }),
            ),
        ],
    );
    pacquet(&workspace, ["install"]).assert().success();

    anchor.set_dist_tag(FOO, "100.1.0", "latest");
    anchor.set_dist_tag(BAR, "100.1.0", "latest");

    pacquet(&workspace, ["-r", "update", "--prod", "--no-optional"]).assert().success();

    assert_eq!(
        lockfile_package_keys(&workspace),
        [format!("{BAR}@100.0.0"), format!("{FOO}@100.1.0")],
    );
    let modules = read_modules_manifest::<ModulesHost>(&workspace.join("node_modules"))
        .expect("read .modules.yaml")
        .expect(".modules.yaml exists");
    assert_eq!(
        modules.included,
        IncludedDependencies {
            dependencies: true,
            dev_dependencies: true,
            optional_dependencies: true,
        },
    );

    drop((root, anchor));
}

/// The rendered stdout+stderr of `pacquet` run in `workspace` with `args`,
/// alongside its exit status.
fn pacquet_output(workspace: &Path, args: &[&str]) -> (std::process::ExitStatus, String) {
    let output = pacquet(workspace, args).output().expect("run pacquet");
    let rendered = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    eprintln!("STATUS: {}\nOUTPUT:\n{rendered}", output.status);
    (output.status, rendered)
}

/// A versioned selector that matches no direct dependency reaches its target
/// through the resolver alone, where the version has nowhere to be recorded.
/// Resolving to something else and exiting 0 would leave the caller nothing to
/// read, so this fails.
#[test]
fn recursive_update_rejects_a_version_for_a_transitive_only_selector() {
    let (root, workspace, anchor) = setup();

    write_workspace(
        &workspace,
        &[(
            "project-1",
            json!({ "name": "project-1", "version": "1.0.0",
            "dependencies": { PKG_WITH_DEP: "100.0.0" } }),
        )],
    );
    pacquet(&workspace, ["install"]).assert().success();

    let (status, rendered) =
        pacquet_output(&workspace, &["-r", "update", &format!("{DEP}@100.1.0")]);

    assert!(!status.success(), "a version that cannot be recorded should fail the command");
    assert!(
        rendered.contains("ERR_PNPM_UPDATE_VERSION_ON_INDIRECT_DEP"),
        "the failure must carry the UPDATE_VERSION_ON_INDIRECT_DEP code",
    );
    // miette wraps the message, so assert on fragments that survive a line break.
    assert!(
        rendered.contains(&format!(r#""{DEP}" (requested "100.1.0")"#)),
        "the failure must name the selector and the version it could not record",
    );
    assert!(
        rendered.contains(&format!("{DEP}@<declared range>: 100.1.0")),
        "the failure must show the override that does pin a transitive dependency",
    );

    drop((root, anchor));
}

/// A selector the workspace declares directly somewhere is legitimately
/// versioned, even where a sibling project only reaches it transitively.
#[test]
fn recursive_update_accepts_a_version_declared_by_any_project() {
    let (root, workspace, anchor) = setup();

    write_workspace(
        &workspace,
        &[
            (
                "project-1",
                json!({ "name": "project-1", "version": "1.0.0",
                "dependencies": { PKG_WITH_DEP: "100.0.0" } }),
            ),
            (
                "project-2",
                json!({ "name": "project-2", "version": "1.0.0",
                "dependencies": { DEP: "100.0.0" } }),
            ),
        ],
    );
    pacquet(&workspace, ["install"]).assert().success();

    let (status, rendered) =
        pacquet_output(&workspace, &["-r", "update", &format!("{DEP}@100.1.0")]);

    assert!(status.success(), "project-2 declares it, so the version has somewhere to go");
    assert!(
        !rendered.contains("ERR_PNPM_UPDATE_VERSION_ON_INDIRECT_DEP"),
        "a selector declared by any project must not be rejected",
    );
    assert_eq!(
        installed_version(&workspace.join("project-2"), DEP).as_deref(),
        Some("100.1.0"),
        "the declaring project should have been updated",
    );

    drop((root, anchor));
}

/// A selector that names no single version -- a tag or a range -- has nothing
/// to record either, but updating within the dependents' ranges is a
/// reasonable reading of it. Those warn rather than fail.
#[test]
fn recursive_update_allows_a_tag_for_a_transitive_only_selector() {
    let (root, workspace, anchor) = setup();

    write_workspace(
        &workspace,
        &[(
            "project-1",
            json!({ "name": "project-1", "version": "1.0.0",
            "dependencies": { PKG_WITH_DEP: "100.0.0" } }),
        )],
    );
    pacquet(&workspace, ["install"]).assert().success();

    let (status, rendered) =
        pacquet_output(&workspace, &["-r", "update", &format!("{DEP}@latest")]);

    assert!(status.success(), "a tag is not a version that has to be recorded");
    assert!(
        rendered.contains(&format!(r#""{DEP}" is not a direct dependency"#))
            && rendered.contains(r#"the requested "latest" is ignored"#),
        "the user should still be told the tag had no effect",
    );

    drop((root, anchor));
}

/// Ports `recursive update with pattern`.
#[test]
fn recursive_update_with_pattern() {
    let (root, workspace, anchor) = setup_with_own_registry();

    write_workspace(
        &workspace,
        &[
            (
                "project-1",
                json!({ "name": "project-1", "version": "1.0.0",
                "dependencies": { DEP: "100.0.0", FOO: "1.0.0" } }),
            ),
            (
                "project-2",
                json!({ "name": "project-2", "version": "1.0.0",
                "dependencies": { PEER_C: "1.0.0" } }),
            ),
        ],
    );
    pacquet(&workspace, ["install"]).assert().success();

    anchor.set_dist_tag(DEP, "100.1.0", "latest");
    anchor.set_dist_tag(PEER_C, "2.0.0", "latest");
    anchor.set_dist_tag(FOO, "2.0.0", "latest");

    pacquet(&workspace, ["-r", "update", "--latest", "@pnpm.e2e/peer-*", "@pnpm.e2e/dep-of-pkg-*"])
        .assert()
        .success();

    let project_1 = workspace.join("project-1");
    let project_2 = workspace.join("project-2");
    assert_eq!(installed_version(&project_1, DEP).as_deref(), Some("100.1.0"));
    assert_eq!(installed_version(&project_1, FOO).as_deref(), Some("1.0.0"));
    assert_eq!(installed_version(&project_2, PEER_C).as_deref(), Some("2.0.0"));

    drop((root, anchor));
}

/// Ports `recursive update with pattern and name in project`.
#[test]
fn recursive_update_with_pattern_and_name_in_project() {
    let (root, workspace, anchor) = setup_with_own_registry();
    anchor.set_dist_tag(DEP, "100.1.0", "latest");
    anchor.set_dist_tag(PEER_C, "2.0.0", "latest");
    anchor.set_dist_tag(FOO, "2.0.0", "latest");
    anchor.set_dist_tag(PRINT_VERSION, "2.0.0", "latest");

    write_workspace(
        &workspace,
        &[
            (
                "project-1",
                json!({ "name": "project-1", "version": "1.0.0",
                "dependencies": { DEP: "100.0.0", FOO: "1.0.0" } }),
            ),
            (
                "project-2",
                json!({ "name": "project-2", "version": "1.0.0",
                "dependencies": { PEER_C: "1.0.0", PRINT_VERSION: "1.0.0" } }),
            ),
        ],
    );
    pacquet(&workspace, ["install"]).assert().success();

    let output = pacquet(
        &workspace,
        ["-r", "update", "--depth", "0", "--latest", "@pnpm.e2e/this-does-not-exist"],
    )
    .output()
    .expect("run pacquet update");
    let stderr = String::from_utf8_lossy(&output.stderr);
    eprintln!("STATUS: {}\nSTDERR:\n{stderr}", output.status);
    assert!(!output.status.success(), "updating an undeclared package should fail");
    assert!(stderr.contains("ERR_PNPM_NO_PACKAGE_IN_DEPENDENCIES"), "{stderr}");

    // Without `--depth 0` the same selector is simply a no-op.
    pacquet(&workspace, ["-r", "update", "--latest", "@pnpm.e2e/this-does-not-exist"])
        .assert()
        .success();

    pacquet(
        &workspace,
        ["-r", "update", "--latest", "@pnpm.e2e/peer-*", "@pnpm.e2e/dep-of-pkg-*", PRINT_VERSION],
    )
    .assert()
    .success();

    let project_1 = workspace.join("project-1");
    let project_2 = workspace.join("project-2");
    assert_eq!(installed_version(&project_1, DEP).as_deref(), Some("100.1.0"));
    assert_eq!(installed_version(&project_1, FOO).as_deref(), Some("1.0.0"));
    assert_eq!(installed_version(&project_2, PEER_C).as_deref(), Some("2.0.0"));
    assert_eq!(installed_version(&project_2, PRINT_VERSION).as_deref(), Some("2.0.0"));

    drop((root, anchor));
}

/// Ports `recursive update --latest foo should only update projects that
/// have foo`, over one lockfile for the whole workspace.
#[test]
fn recursive_update_latest_only_reaches_the_named_packages() {
    let (root, workspace, anchor) = setup_with_own_registry();
    anchor.set_dist_tag(FOO, "100.0.0", "latest");
    anchor.set_dist_tag(BAR, "100.0.0", "latest");
    anchor.set_dist_tag(QAR, "100.0.0", "latest");
    anchor.set_dist_tag(MULTI_VERSION_B, "1.0.0", "latest");

    write_workspace(
        &workspace,
        &[
            (
                "project-1",
                json!({ "name": "project-1", "version": "1.0.0",
                "dependencies": { FOO: "100.0.0", QAR: "100.0.0" } }),
            ),
            (
                "project-2",
                json!({ "name": "project-2", "version": "1.0.0",
                "dependencies": { MULTI_VERSION_B: "1.0.0", BAR: "^100.0.0" } }),
            ),
        ],
    );
    pacquet(&workspace, ["install"]).assert().success();

    anchor.set_dist_tag(FOO, "100.1.0", "latest");
    anchor.set_dist_tag(BAR, "100.1.0", "latest");
    anchor.set_dist_tag(MULTI_VERSION_B, "3.1.0", "latest");

    pacquet(&workspace, ["-r", "update", "--latest", MULTI_VERSION_B, FOO]).assert().success();

    assert_eq!(
        lockfile_package_keys(&workspace),
        [
            format!("{BAR}@100.0.0"),
            format!("{FOO}@100.1.0"),
            format!("{MULTI_VERSION_B}@3.1.0"),
            format!("{QAR}@100.0.0"),
        ],
    );

    drop((root, anchor));
}

/// At `--depth 0` a transitive dependency is never traversed, so a selector
/// that names one is out of scope rather than an unrecordable request — even
/// alongside a selector that does match a direct dependency.
#[test]
fn recursive_update_depth_zero_leaves_an_indirect_selector_out_of_scope() {
    let (root, workspace, anchor) = setup();

    write_workspace(
        &workspace,
        &[(
            "project-1",
            json!({ "name": "project-1", "version": "1.0.0",
            "dependencies": { PKG_WITH_DEP: "100.0.0", FOO: "100.0.0" } }),
        )],
    );
    pacquet(&workspace, ["install"]).assert().success();

    let (status, rendered) = pacquet_output(
        &workspace,
        &["-r", "update", "--depth", "0", &format!("{FOO}@100.0.0"), &format!("{DEP}@100.1.0")],
    );

    assert!(status.success(), "an untraversed selector must not fail the command");
    assert!(
        !rendered.contains("ERR_PNPM_UPDATE_VERSION_ON_INDIRECT_DEP"),
        "depth 0 leaves the indirect selector out of scope",
    );

    drop((root, anchor));
}

/// Ports `recursive update --latest foo should only update packages that
/// have foo`, over a lockfile per project.
#[test]
fn recursive_update_latest_with_dedicated_lockfiles_only_touches_the_declaring_project() {
    let (root, workspace, anchor) = setup_with_own_registry();
    anchor.set_dist_tag(FOO, "100.0.0", "latest");
    anchor.set_dist_tag(BAR, "100.0.0", "latest");
    anchor.set_dist_tag(QAR, "100.0.0", "latest");
    append_workspace_yaml_key(&workspace, "sharedWorkspaceLockfile", false);

    write_workspace(
        &workspace,
        &[
            (
                "project-1",
                json!({ "name": "project-1", "version": "1.0.0",
                "dependencies": { FOO: "100.0.0", QAR: "100.0.0" } }),
            ),
            (
                "project-2",
                json!({ "name": "project-2", "version": "1.0.0",
                "dependencies": { BAR: "^100.0.0" } }),
            ),
        ],
    );
    pacquet(&workspace, ["install"]).assert().success();

    anchor.set_dist_tag(FOO, "100.1.0", "latest");
    anchor.set_dist_tag(BAR, "100.1.0", "latest");

    pacquet(&workspace, ["-r", "update", "--latest", FOO]).assert().success();

    assert_eq!(
        lockfile_package_keys(&workspace.join("project-1")),
        [format!("{FOO}@100.1.0"), format!("{QAR}@100.0.0")],
    );
    assert_eq!(lockfile_package_keys(&workspace.join("project-2")), [format!("{BAR}@100.0.0")]);

    drop((root, anchor));
}

/// `--latest` rejects every versioned selector on its own, direct or not, and
/// has to report that ahead of the indirect-version check.
#[test]
fn recursive_update_latest_reports_the_spec_ban_first() {
    let (root, workspace, anchor) = setup();

    write_workspace(
        &workspace,
        &[(
            "project-1",
            json!({ "name": "project-1", "version": "1.0.0",
            "dependencies": { PKG_WITH_DEP: "100.0.0" } }),
        )],
    );
    pacquet(&workspace, ["install"]).assert().success();

    let (status, rendered) =
        pacquet_output(&workspace, &["-r", "update", "--latest", &format!("{DEP}@100.1.0")]);

    assert!(!status.success(), "a versioned selector with --latest should fail");
    assert!(
        rendered.contains("ERR_PNPM_LATEST_WITH_SPEC"),
        "--latest owns this failure: {rendered}",
    );
    assert!(
        !rendered.contains("ERR_PNPM_UPDATE_VERSION_ON_INDIRECT_DEP"),
        "the indirect-version check must not preempt it",
    );

    drop((root, anchor));
}
