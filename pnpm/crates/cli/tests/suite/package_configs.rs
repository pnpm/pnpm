//! `packageConfigs`: settings a workspace declares for one project
//! rather than for all of them.

pub use _utils::*;

use crate::_utils;

use indexmap::IndexMap;
use pnpm_modules_yaml::{Host as ModulesHost, read_modules_manifest};
use pretty_assertions::assert_eq;
use std::{
    fs,
    path::{Path, PathBuf},
};

const DEP: &str = "@pnpm.e2e/dep-of-pkg-with-1-dep";
const PARENT: &str = "@pnpm.e2e/pkg-with-1-dep";
const HELLO: &str = "@pnpm.e2e/hello-world-js-bin";

/// Assert the project at `project` resolved exactly one `DEP`, at
/// `version`. `PARENT` declares `^100.0.0`, which the registry serves
/// 100.0.0 and 100.1.0 for, so the pinned and unpinned answers differ.
fn assert_resolved_dep(project: &Path, version: &str) {
    let lockfile = read_lockfile(&project.join("pnpm-lock.yaml"));
    let keys: Vec<String> = snapshot_entries(&lockfile, DEP)
        .into_iter()
        .map(|(key, _)| key)
        .collect();
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

/// The workspace manifest is read wherever the command runs, so
/// narrowing the install to one project does not narrow the settings it
/// sees.
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
        moved
            .join("modules")
            .join(PARENT)
            .is_symlink(),
        "the moved project links into modules",
    );
    assert!(!has_link(&moved, PARENT), "the moved project must not link into node_modules");
    assert!(has_link(&stayed, PARENT));
}

#[test]
fn save_exact_and_save_prefix_apply_to_the_named_projects_only() {
    let fixture = dedicated_lockfile_workspace(
        "packageConfigs:\n  exact:\n    saveExact: true\n  tilde:\n    savePrefix: \"~\"\n",
    );
    let exact = fixture.project("exact", "exact", ManifestDeps::default());
    let tilde = fixture.project("tilde", "tilde", ManifestDeps::default());
    let ranged = fixture.project("ranged", "ranged", ManifestDeps::default());

    for project in [&exact, &tilde, &ranged] {
        fixture.run_at(project, ["add", PARENT]);
    }

    let exact_spec = dependency_spec(&exact, "dependencies", PARENT).expect("exact saved a spec");
    assert_eq!(dependency_spec(&tilde, "dependencies", PARENT), Some(format!("~{exact_spec}")));
    assert_eq!(dependency_spec(&ranged, "dependencies", PARENT), Some(format!("^{exact_spec}")));
}

/// Workspace discovery accepts a `package.yaml` project, so the name the
/// settings are addressed by has to come from whichever manifest the
/// project actually has.
#[test]
fn a_package_yaml_project_gets_its_settings() {
    let fixture = dedicated_lockfile_workspace(&format!(
        "packageConfigs:\n  pinned:\n    overrides:\n      \"{DEP}\": 100.0.0\n",
    ));
    let pinned = fixture.workspace.join("packages/pinned");
    fs::create_dir_all(&pinned).unwrap();
    fs::write(
        pinned.join("package.yaml"),
        format!("name: pinned\nversion: 1.0.0\ndependencies:\n  \"{PARENT}\": 100.0.0\n"),
    )
    .unwrap();

    fixture.run(["install"]);

    assert_resolved_dep(&pinned, "100.0.0");
}

/// A `--filter`ed run reaches its projects through a different plan than
/// an unfiltered one.
#[test]
fn a_filtered_recursive_install_applies_the_entry() {
    let fixture = dedicated_lockfile_workspace(&format!(
        "packageConfigs:\n  pinned:\n    overrides:\n      \"{DEP}\": 100.0.0\n",
    ));
    let pinned = fixture.project(
        "pinned",
        "pinned",
        ManifestDeps { prod: &[(PARENT, "100.0.0")], ..Default::default() },
    );
    let other = fixture.project(
        "other",
        "other",
        ManifestDeps { prod: &[(PARENT, "100.0.0")], ..Default::default() },
    );

    fixture.run(["--filter", "pinned", "install"]);

    assert_resolved_dep(&pinned, "100.0.0");
    assert!(!other.join("pnpm-lock.yaml").exists(), "an unselected project is not installed");
}

/// The workspace root installs with the projects it declares but is not
/// one of them.
#[test]
fn the_workspace_root_gets_its_own_entry() {
    let fixture = dedicated_lockfile_workspace(&format!(
        "packageConfigs:\n  root:\n    overrides:\n      \"{DEP}\": 100.0.0\n",
    ));
    fixture.write_root_manifest(
        "root",
        ManifestDeps { prod: &[(PARENT, "100.0.0")], ..Default::default() },
    );
    let child = fixture.project(
        "child",
        "child",
        ManifestDeps { prod: &[(PARENT, "100.0.0")], ..Default::default() },
    );

    fixture.run(["install"]);

    assert_resolved_dep(&fixture.workspace, "100.0.0");
    assert_resolved_dep(&child, "100.1.0");
}

/// `update` re-resolves, so the entry has to hold through it too.
#[test]
fn update_keeps_the_entry_overrides() {
    let fixture = dedicated_lockfile_workspace(&format!(
        "packageConfigs:\n  pinned:\n    overrides:\n      \"{DEP}\": 100.0.0\n",
    ));
    let pinned = fixture.project(
        "pinned",
        "pinned",
        ManifestDeps { prod: &[(PARENT, "^100.0.0")], ..Default::default() },
    );
    let other = fixture.project(
        "other",
        "other",
        ManifestDeps { prod: &[(PARENT, "^100.0.0")], ..Default::default() },
    );

    for project in [&pinned, &other] {
        fixture.run_at(project, ["update", "--latest"]);
    }

    assert_resolved_dep(&pinned, "100.0.0");
    assert_resolved_dep(&other, "100.1.0");
}

/// `remove` re-resolves what is left, so the entry has to hold through
/// it too.
#[test]
fn remove_keeps_the_entry_overrides() {
    let fixture = dedicated_lockfile_workspace(&format!(
        "packageConfigs:\n  pinned:\n    overrides:\n      \"{DEP}\": 100.0.0\n",
    ));
    let pinned = fixture.project(
        "pinned",
        "pinned",
        ManifestDeps { prod: &[(PARENT, "100.0.0"), (HELLO, "1.0.0")], ..Default::default() },
    );

    fixture.run_at(&pinned, ["install"]);
    fixture.run_at(&pinned, ["remove", HELLO]);

    assert_resolved_dep(&pinned, "100.0.0");
    assert!(dependency_spec(&pinned, "dependencies", HELLO).is_none());
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

/// A workspace sharing one lockfile resolves every project together, so
/// a per-project setting has no install of its own to reach.
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

fn bin_dir(fixture: &WorkspaceFixture, project: &Path) -> PathBuf {
    let output = fixture.command_at(project, ["bin"]);
    assert_success(&output);
    PathBuf::from(String::from_utf8_lossy(&output.stdout).trim_end())
}

fn canonical_bin_dir(project: &Path, modules_dir: &str) -> PathBuf {
    dunce::canonicalize(project)
        .expect("canonicalize the project dir")
        .join(modules_dir)
        .join(".bin")
}

const MOVED_MODULES: &str =
    "modulesDir: vendor\npackageConfigs:\n  moved:\n    modulesDir: node_modules\n";

#[cfg(unix)]
fn greeter(fixture: &WorkspaceFixture, dir: &str, name: &str) -> PathBuf {
    let project = fixture.workspace.join("packages").join(dir);
    fs::create_dir_all(&project).expect("create the project dir");
    fs::write(
        project.join("package.json"),
        format!(r#"{{ "name": "{name}", "version": "1.0.0", "scripts": {{ "greet": "greet" }} }}"#),
    )
    .expect("write package.json");
    project
}

#[cfg(unix)]
fn write_greeter_shim(modules_dir: &Path, marker: &str) {
    let bin_dir = modules_dir.join(".bin");
    fs::create_dir_all(&bin_dir).expect("create the bin dir");
    write_executable(&bin_dir.join("greet"), &format!("#!/bin/sh\necho {marker}\n"));
}

/// Regression test for the per-project half of
/// [pnpm/pnpm#3604](https://github.com/pnpm/pnpm/issues/3604). The install
/// gives each project the modules directory its entry names, including one
/// that names the default back, so `bin` has to report the same one.
#[test]
fn bin_reports_the_named_project_modules_dir() {
    let fixture = dedicated_lockfile_workspace(
        "modulesDir: vendor\npackageConfigs:\n  back:\n    modulesDir: node_modules\n  aside:\n    modulesDir: private_modules\n",
    );
    let back = fixture.project("back", "back", ManifestDeps::default());
    let aside = fixture.project("aside", "aside", ManifestDeps::default());
    let plain = fixture.project("plain", "plain", ManifestDeps::default());

    assert_eq!(bin_dir(&fixture, &back), canonical_bin_dir(&back, "node_modules"));
    assert_eq!(bin_dir(&fixture, &aside), canonical_bin_dir(&aside, "private_modules"));
    assert_eq!(bin_dir(&fixture, &plain), canonical_bin_dir(&plain, "vendor"));
}

/// `root` reports the modules directory the install gave the project, the
/// one its `packageConfigs` entry names.
#[test]
fn root_reports_the_named_project_modules_dir() {
    let fixture = dedicated_lockfile_workspace(
        "modulesDir: vendor\npackageConfigs:\n  aside:\n    modulesDir: private_modules\n",
    );
    let aside = fixture.project("aside", "aside", ManifestDeps::default());
    let plain = fixture.project("plain", "plain", ManifestDeps::default());

    for (project, modules_dir) in [(&aside, "private_modules"), (&plain, "vendor")] {
        let output = fixture.command_at(project, ["root"]);
        assert_success(&output);
        let expected =
            dunce::canonicalize(project).expect("canonicalize the project dir").join(modules_dir);
        assert_eq!(PathBuf::from(String::from_utf8_lossy(&output.stdout).trim_end()), expected);
    }
}

/// The entries are inert under a shared lockfile, so a command must not
/// read one back either.
#[test]
fn a_shared_lockfile_keeps_the_workspace_modules_dir() {
    let fixture = WorkspaceFixture::new();
    fixture.append_workspace_yaml(MOVED_MODULES);
    let moved = fixture.project("moved", "moved", ManifestDeps::default());

    assert_eq!(bin_dir(&fixture, &moved), canonical_bin_dir(&moved, "vendor"));
}

#[cfg(unix)]
#[test]
fn run_and_exec_find_the_named_project_command() {
    let fixture = dedicated_lockfile_workspace(MOVED_MODULES);
    let moved = greeter(&fixture, "moved", "moved");
    write_greeter_shim(&moved.join("node_modules"), "configured");
    write_greeter_shim(&moved.join("vendor"), "stale");

    for args in [["run", "greet"], ["exec", "greet"]] {
        let output = fixture.command_at(&moved, args);
        assert_success(&output);
        let stdout = String::from_utf8_lossy(&output.stdout);
        println!("{stdout}");
        assert!(stdout.contains("configured"), "{args:?} must use the entry's dir: {stdout}");
        assert!(!stdout.contains("stale"), "{args:?} must not use the workspace dir: {stdout}");
    }
}

/// The workspace root is a project like any other: its entry moves the
/// executables every member reaches through `extraBinPaths`.
#[cfg(unix)]
#[test]
fn the_workspace_root_entry_moves_the_shared_executables() {
    let fixture = dedicated_lockfile_workspace(
        "modulesDir: vendor\npackageConfigs:\n  wsroot:\n    modulesDir: node_modules\n",
    );
    fixture.write_root_manifest("wsroot", ManifestDeps::default());
    let member = greeter(&fixture, "member", "member");
    write_greeter_shim(&fixture.workspace.join("node_modules"), "configured");
    write_greeter_shim(&fixture.workspace.join("vendor"), "stale");

    let output = fixture.command_at(&member, ["run", "greet"]);
    assert_success(&output);
    let stdout = String::from_utf8_lossy(&output.stdout);
    println!("{stdout}");
    assert!(stdout.contains("configured"), "the root entry must move extraBinPaths: {stdout}");
    assert!(!stdout.contains("stale"), "the workspace-wide dir must not win: {stdout}");
}

#[cfg(unix)]
#[test]
fn a_recursive_exec_uses_each_project_modules_dir() {
    let fixture = dedicated_lockfile_workspace(
        "modulesDir: vendor\npackageConfigs:\n  one:\n    modulesDir: node_modules\n  two:\n    modulesDir: private_modules\n",
    );
    let one = fixture.project("one", "one", ManifestDeps::default());
    let two = fixture.project("two", "two", ManifestDeps::default());
    write_greeter_shim(&one.join("node_modules"), "from_one");
    write_greeter_shim(&two.join("private_modules"), "from_two");

    let output = fixture.command_at(&fixture.workspace, ["--recursive", "exec", "greet"]);
    assert_success(&output);
    let stdout = String::from_utf8_lossy(&output.stdout);
    println!("{stdout}");
    assert!(stdout.contains("from_one"), "{stdout}");
    assert!(stdout.contains("from_two"), "{stdout}");
}
