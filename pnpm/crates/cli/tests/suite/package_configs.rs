//! `packageConfigs`: settings a workspace declares for one project
//! rather than for all of them.

use crate::_utils;
pub use _utils::*;

use indexmap::IndexMap;
use pnpm_modules_yaml::{Host as ModulesHost, read_modules_manifest};
use pretty_assertions::assert_eq;
use std::path::Path;

const DEP: &str = "@pnpm.e2e/dep-of-pkg-with-1-dep";
const PARENT: &str = "@pnpm.e2e/pkg-with-1-dep";

/// Assert the project at `project` resolved exactly one `DEP`, at
/// `version`. `PARENT` declares `^100.0.0`, which the registry serves
/// 100.0.0 and 100.1.0 for, so the pinned and unpinned answers differ.
fn assert_resolved_dep(project: &Path, version: &str) {
    let lockfile = read_lockfile(&project.join("pnpm-lock.yaml"));
    let keys: Vec<String> =
        snapshot_entries(&lockfile, DEP).into_iter().map(|(key, _)| key).collect();
    dbg!(&keys);
    assert_eq!(keys, vec![format!("{DEP}@{version}")], "in {}", project.display());
}

fn lockfile_overrides(project: &Path) -> Option<IndexMap<String, String>> {
    read_lockfile(&project.join("pnpm-lock.yaml")).overrides
}

fn dedicated_lockfile_workspace(package_configs: &str) -> WorkspaceFixture {
    let fixture = WorkspaceFixture::new();
    fixture.append_workspace_yaml("sharedWorkspaceLockfile: false\n");
    fixture.append_workspace_yaml(package_configs);
    fixture
}

/// <https://github.com/pnpm/pnpm/issues/14556>
#[test]
fn overrides_apply_to_the_named_project_only() {
    let fixture = dedicated_lockfile_workspace(&format!(
        "packageConfigs:\n  pinned:\n    overrides:\n      \"{DEP}\": 100.0.0\n",
    ));
    let pinned = fixture.project(
        "pinned",
        "pinned",
        ManifestDeps { prod: &[(PARENT, "100.0.0")], ..Default::default() },
    );
    let unpinned = fixture.project(
        "unpinned",
        "unpinned",
        ManifestDeps { prod: &[(PARENT, "100.0.0")], ..Default::default() },
    );

    fixture.run(["install"]);

    assert_resolved_dep(&pinned, "100.0.0");
    assert_eq!(
        lockfile_overrides(&pinned),
        Some(IndexMap::from([(DEP.to_string(), "100.0.0".to_string())])),
    );
    assert_resolved_dep(&unpinned, "100.1.0");
    assert_eq!(lockfile_overrides(&unpinned), None);
}

/// The list form maps one settings block onto several projects.
#[test]
fn the_list_form_overrides_every_matched_project() {
    let fixture = dedicated_lockfile_workspace(&format!(
        "packageConfigs:\n  - match:\n      - first\n      - second\n    overrides:\n      \"{DEP}\": 100.0.0\n",
    ));
    let first = fixture.project(
        "first",
        "first",
        ManifestDeps { prod: &[(PARENT, "100.0.0")], ..Default::default() },
    );
    let second = fixture.project(
        "second",
        "second",
        ManifestDeps { prod: &[(PARENT, "100.0.0")], ..Default::default() },
    );
    let third = fixture.project(
        "third",
        "third",
        ManifestDeps { prod: &[(PARENT, "100.0.0")], ..Default::default() },
    );

    fixture.run(["install"]);

    assert_resolved_dep(&first, "100.0.0");
    assert_resolved_dep(&second, "100.0.0");
    assert_resolved_dep(&third, "100.1.0");
}

/// A project the setting names keeps its own overrides when the
/// install is narrowed to it, because the workspace manifest is read
/// wherever the command runs.
#[test]
fn overrides_apply_to_a_filtered_install_of_the_project() {
    let fixture = dedicated_lockfile_workspace(&format!(
        "packageConfigs:\n  pinned:\n    overrides:\n      \"{DEP}\": 100.0.0\n",
    ));
    let pinned = fixture.project(
        "pinned",
        "pinned",
        ManifestDeps { prod: &[(PARENT, "100.0.0")], ..Default::default() },
    );

    fixture.run_at(&pinned, ["install"]);

    assert_resolved_dep(&pinned, "100.0.0");
}

/// Port of upstream's `recursive installation with packageConfigs`.
#[test]
fn hoist_false_disables_hoisting_for_the_named_project_only() {
    let fixture = dedicated_lockfile_workspace("packageConfigs:\n  unhoisted:\n    hoist: false\n");
    let hoisted = fixture.project(
        "hoisted",
        "hoisted",
        ManifestDeps { prod: &[(PARENT, "100.0.0")], ..Default::default() },
    );
    let unhoisted = fixture.project(
        "unhoisted",
        "unhoisted",
        ManifestDeps { prod: &[(PARENT, "100.0.0")], ..Default::default() },
    );

    fixture.run(["install"]);

    assert_eq!(project_hoist_pattern(&hoisted), Some(vec!["*".to_string()]));
    assert_eq!(project_hoist_pattern(&unhoisted), None);
}

fn project_hoist_pattern(project: &Path) -> Option<Vec<String>> {
    read_modules_manifest::<ModulesHost>(&project.join("node_modules"))
        .expect("read .modules.yaml")
        .expect(".modules.yaml exists")
        .hoist_pattern
}

#[test]
fn modules_dir_moves_only_the_named_project() {
    let fixture =
        dedicated_lockfile_workspace("packageConfigs:\n  moved:\n    modulesDir: modules\n");
    let moved = fixture.project(
        "moved",
        "moved",
        ManifestDeps { prod: &[(PARENT, "100.0.0")], ..Default::default() },
    );
    let stayed = fixture.project(
        "stayed",
        "stayed",
        ManifestDeps { prod: &[(PARENT, "100.0.0")], ..Default::default() },
    );

    fixture.run(["install"]);

    assert!(
        moved.join("modules").join(PARENT).is_symlink(),
        "the moved project links into modules",
    );
    assert!(!has_link(&moved, PARENT), "the moved project must not link into node_modules");
    assert!(has_link(&stayed, PARENT));
}

#[test]
fn save_exact_applies_to_the_named_project_only() {
    let fixture = dedicated_lockfile_workspace("packageConfigs:\n  exact:\n    saveExact: true\n");
    let exact = fixture.project("exact", "exact", ManifestDeps::default());
    let ranged = fixture.project("ranged", "ranged", ManifestDeps::default());

    fixture.run_at(&exact, ["add", PARENT]);
    fixture.run_at(&ranged, ["add", PARENT]);

    let exact_spec = dependency_spec(&exact, "dependencies", PARENT).expect("exact saved a spec");
    let ranged_spec =
        dependency_spec(&ranged, "dependencies", PARENT).expect("ranged saved a spec");
    assert_eq!(ranged_spec, format!("^{exact_spec}"));
}

#[test]
fn config_get_reports_the_setting() {
    let fixture = WorkspaceFixture::new();
    fixture.append_workspace_yaml("packageConfigs:\n  a:\n    saveExact: true\n");

    let output = fixture.command_at(&fixture.workspace, ["config", "get", "packageConfigs"]);
    assert_success(&output);
    let stdout = String::from_utf8_lossy(&output.stdout);
    println!("{stdout}");
    assert!(stdout.contains("saveExact"), "{stdout}");
}

#[test]
fn an_unsupported_setting_is_rejected() {
    let fixture = WorkspaceFixture::new();
    fixture.append_workspace_yaml("packageConfigs:\n  a:\n    saveExactly: true\n");
    fixture.project("a", "a", ManifestDeps::default());

    let output = fixture.command_at(&fixture.workspace, ["install"]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    println!("{stderr}");
    assert!(!output.status.success(), "an unsupported packageConfigs field must fail the install");
    assert!(stderr.contains("saveExactly"), "{stderr}");
}

/// A workspace whose projects share one lockfile resolves them
/// together, so a per-project setting has no install of its own to
/// reach. The entries are inert there, and the install says so.
#[test]
fn a_shared_lockfile_reports_the_ignored_settings() {
    let fixture = WorkspaceFixture::new();
    fixture.append_workspace_yaml(&format!(
        "packageConfigs:\n  pinned:\n    overrides:\n      \"{DEP}\": 100.0.0\n    saveExact: true\n",
    ));
    fixture.project(
        "pinned",
        "pinned",
        ManifestDeps { prod: &[(PARENT, "100.0.0")], ..Default::default() },
    );

    let output = fixture.command_at(&fixture.workspace, ["install"]);
    assert_success(&output);
    let stderr = String::from_utf8_lossy(&output.stderr);
    println!("{stderr}");
    assert!(stderr.contains(r#""pinned.overrides", "pinned.saveExact""#), "{stderr}");

    assert_eq!(fixture.wanted().overrides, None);
    assert!(has_snapshot(&fixture.wanted(), DEP, "100.1.0"));
}

/// The projects of a dedicated-lockfile workspace each run their own
/// install, so nothing is ignored and nothing is reported.
#[test]
fn dedicated_lockfiles_report_no_ignored_settings() {
    let fixture = dedicated_lockfile_workspace(&format!(
        "packageConfigs:\n  pinned:\n    overrides:\n      \"{DEP}\": 100.0.0\n",
    ));
    fixture.project(
        "pinned",
        "pinned",
        ManifestDeps { prod: &[(PARENT, "100.0.0")], ..Default::default() },
    );

    let output = fixture.command_at(&fixture.workspace, ["install"]);
    assert_success(&output);
    let stderr = String::from_utf8_lossy(&output.stderr);
    println!("{stderr}");
    assert!(!stderr.contains("packageConfigs"), "{stderr}");
}
