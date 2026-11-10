//! Installing with `lockfile.format: v13`: the lockfile pnpm writes, a
//! frozen install from it, and 3-way `git merge-file` merges across
//! concurrent branch changes.

use crate::_utils::pacquet_in;
use assert_cmd::assert::OutputAssertExt;
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::CommandTempCwd;
use serde_json::{Value, json};
use std::{fs, path::Path, process::Command};

#[test]
fn install_writes_the_v13_lockfile_and_a_frozen_install_reads_it() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init().add_mocked_registry();
    fs::write(workspace.join("pnpm-workspace.yaml"), "lockfile:\n  format: v13\n")
        .expect("write pnpm-workspace.yaml");
    let manifest = json!({
        "name": "v13-project",
        "version": "0.0.0",
        "dependencies": {
            "@pnpm.e2e/pkg-with-1-dep": "100.0.0",
        },
    });
    fs::write(workspace.join("package.json"), manifest.to_string()).expect("write package.json");

    pacquet_in(&workspace)
        .with_args(["install", "--reporter=silent"])
        .assert()
        .success();

    let lockfile_path = workspace.join("pnpm-lock.yaml");
    let lockfile = fs::read_to_string(&lockfile_path).expect("read the lockfile");
    assert!(lockfile.starts_with("{\n\t\"lockfileVersion\": \"13.0\",\n"), "{lockfile}");
    assert!(lockfile.contains("\n\t\t\"@pnpm.e2e/pkg-with-1-dep@100.0.0\": {\n"), "{lockfile}");
    assert!(
        lockfile.contains("\n\t\t\t\t\"@pnpm.e2e/pkg-with-1-dep\": \"100.0.0\",\n"),
        "{lockfile}",
    );
    assert!(!lockfile.contains("snapshots"), "{lockfile}");

    fs::remove_dir_all(workspace.join("node_modules")).expect("remove node_modules");
    pacquet_in(&workspace)
        .with_args(["install", "--frozen-lockfile", "--reporter=silent"])
        .assert()
        .success();

    assert!(workspace.join("node_modules/@pnpm.e2e/pkg-with-1-dep/package.json").exists());
    assert_eq!(fs::read_to_string(&lockfile_path).expect("read the lockfile"), lockfile);

    drop(root);
}

struct BranchState<'a> {
    workspace_yaml: &'a str,
    projects: &'a [(&'a str, Value)],
}

fn write_state(workspace: &Path, state: &BranchState<'_>) {
    let _ = fs::remove_file(workspace.join("pnpm-lock.yaml"));
    let _ = fs::remove_dir_all(workspace.join("node_modules"));
    let _ = fs::remove_dir_all(workspace.join("packages"));
    let workspace_yaml = format!("lockfile:\n  format: v13\n{}", state.workspace_yaml);
    fs::write(workspace.join("pnpm-workspace.yaml"), workspace_yaml)
        .expect("write pnpm-workspace.yaml");
    for (dir, manifest) in state.projects {
        let project_dir = if *dir == "." { workspace.to_path_buf() } else { workspace.join(dir) };
        fs::create_dir_all(&project_dir).expect("create project directory");
        fs::write(project_dir.join("package.json"), manifest.to_string())
            .expect("write package.json");
    }
}

fn install_v13_state(workspace: &Path, state: &BranchState<'_>) -> String {
    write_state(workspace, state);
    pacquet_in(workspace)
        .with_args(["install", "--reporter=silent"])
        .assert()
        .success();
    fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read pnpm-lock.yaml")
}

fn git_merge_lockfiles(temp_dir: &Path, base: &str, branch_a: &str, branch_b: &str) -> String {
    let base_path = temp_dir.join("base.yaml");
    let a_path = temp_dir.join("a.yaml");
    let b_path = temp_dir.join("b.yaml");
    fs::write(&base_path, base).expect("write base lockfile");
    fs::write(&a_path, branch_a).expect("write branch A lockfile");
    fs::write(&b_path, branch_b).expect("write branch B lockfile");
    let output = Command::new("git")
        .args(["merge-file", "-p"])
        .arg(&a_path)
        .arg(&base_path)
        .arg(&b_path)
        .output()
        .expect("run git merge-file");
    let merged = String::from_utf8(output.stdout).expect("utf8 merged lockfile");
    assert!(output.status.success(), "expected clean 3-way git merge, got conflict:\n{merged}");
    merged
}

fn assert_clean_v13_git_merge(
    base: &BranchState<'_>,
    branch_a: &BranchState<'_>,
    branch_b: &BranchState<'_>,
    expected_merged: &BranchState<'_>,
) {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init().add_mocked_registry();
    let base_lockfile = install_v13_state(&workspace, base);
    let a_lockfile = install_v13_state(&workspace, branch_a);
    let b_lockfile = install_v13_state(&workspace, branch_b);
    let expected_lockfile = install_v13_state(&workspace, expected_merged);

    let merged_lockfile =
        git_merge_lockfiles(root.path(), &base_lockfile, &a_lockfile, &b_lockfile);
    assert_eq!(merged_lockfile, expected_lockfile);

    write_state(&workspace, expected_merged);
    fs::write(workspace.join("pnpm-lock.yaml"), &merged_lockfile).expect("write merged lockfile");
    pacquet_in(&workspace)
        .with_args(["install", "--frozen-lockfile", "--reporter=silent"])
        .assert()
        .success();

    drop(root);
}

#[test]
fn git_merge_bumping_adjacent_ranged_dependencies_merges_cleanly() {
    let project = |bar: &str, dep: &str| {
        json!({
            "name": "v13-merge",
            "version": "0.0.0",
            "dependencies": {
                "@pnpm.e2e/bar": bar,
                "@pnpm.e2e/dep-of-pkg-with-1-dep": dep,
            },
        })
    };
    assert_clean_v13_git_merge(
        &BranchState { workspace_yaml: "", projects: &[(".", project("~100.0.0", "~100.0.0"))] },
        &BranchState { workspace_yaml: "", projects: &[(".", project("^100.1.0", "~100.0.0"))] },
        &BranchState { workspace_yaml: "", projects: &[(".", project("~100.0.0", "^100.1.0"))] },
        &BranchState { workspace_yaml: "", projects: &[(".", project("^100.1.0", "^100.1.0"))] },
    );
}

#[test]
fn git_merge_bumping_ranged_dependency_and_adding_adjacent_dependency_merges_cleanly() {
    assert_clean_v13_git_merge(
        &BranchState {
            workspace_yaml: "",
            projects: &[(
                ".",
                json!({
                    "name": "v13-merge",
                    "version": "0.0.0",
                    "dependencies": {
                        "@pnpm.e2e/bar": "~100.0.0",
                        "@pnpm.e2e/qar": "100.0.0",
                    },
                }),
            )],
        },
        &BranchState {
            workspace_yaml: "",
            projects: &[(
                ".",
                json!({
                    "name": "v13-merge",
                    "version": "0.0.0",
                    "dependencies": {
                        "@pnpm.e2e/bar": "^100.1.0",
                        "@pnpm.e2e/qar": "100.0.0",
                    },
                }),
            )],
        },
        &BranchState {
            workspace_yaml: "",
            projects: &[(
                ".",
                json!({
                    "name": "v13-merge",
                    "version": "0.0.0",
                    "dependencies": {
                        "@pnpm.e2e/bar": "~100.0.0",
                        "@pnpm.e2e/foo": "100.0.0",
                        "@pnpm.e2e/qar": "100.0.0",
                    },
                }),
            )],
        },
        &BranchState {
            workspace_yaml: "",
            projects: &[(
                ".",
                json!({
                    "name": "v13-merge",
                    "version": "0.0.0",
                    "dependencies": {
                        "@pnpm.e2e/bar": "^100.1.0",
                        "@pnpm.e2e/foo": "100.0.0",
                        "@pnpm.e2e/qar": "100.0.0",
                    },
                }),
            )],
        },
    );
}

#[test]
fn git_merge_bumping_adjacent_catalog_entries_merges_cleanly() {
    let manifest = json!({
        "name": "v13-catalog-merge",
        "version": "0.0.0",
        "dependencies": {
            "@pnpm.e2e/bar": "catalog:",
            "@pnpm.e2e/foo": "catalog:",
        },
    });
    let catalog = |bar: &str, foo: &str| -> String {
        format!("catalog:\n  '@pnpm.e2e/bar': {bar}\n  '@pnpm.e2e/foo': {foo}\n")
    };
    let base_yaml = catalog("~100.0.0", "~100.0.0");
    let a_yaml = catalog("^100.1.0", "~100.0.0");
    let b_yaml = catalog("~100.0.0", "^100.1.0");
    let merged_yaml = catalog("^100.1.0", "^100.1.0");
    assert_clean_v13_git_merge(
        &BranchState { workspace_yaml: &base_yaml, projects: &[(".", manifest.clone())] },
        &BranchState { workspace_yaml: &a_yaml, projects: &[(".", manifest.clone())] },
        &BranchState { workspace_yaml: &b_yaml, projects: &[(".", manifest.clone())] },
        &BranchState { workspace_yaml: &merged_yaml, projects: &[(".", manifest)] },
    );
}

#[test]
fn git_merge_bumping_peer_and_binding_adjacent_optional_peer_merges_cleanly() {
    assert_clean_v13_git_merge(
        &BranchState {
            workspace_yaml: "",
            projects: &[(
                ".",
                json!({
                    "name": "v13-peer-merge",
                    "version": "0.0.0",
                    "dependencies": {
                        "@pnpm.e2e/abc-optional-peers": "1.0.0",
                        "@pnpm.e2e/peer-a": "~1.0.0",
                        "@pnpm.e2e/qar": "100.0.0",
                    },
                }),
            )],
        },
        &BranchState {
            workspace_yaml: "",
            projects: &[(
                ".",
                json!({
                    "name": "v13-peer-merge",
                    "version": "0.0.0",
                    "dependencies": {
                        "@pnpm.e2e/abc-optional-peers": "1.0.0",
                        "@pnpm.e2e/peer-a": "^1.0.1",
                        "@pnpm.e2e/qar": "100.0.0",
                    },
                }),
            )],
        },
        &BranchState {
            workspace_yaml: "",
            projects: &[(
                ".",
                json!({
                    "name": "v13-peer-merge",
                    "version": "0.0.0",
                    "dependencies": {
                        "@pnpm.e2e/abc-optional-peers": "1.0.0",
                        "@pnpm.e2e/peer-a": "~1.0.0",
                        "@pnpm.e2e/peer-b": "1.0.0",
                        "@pnpm.e2e/qar": "100.0.0",
                    },
                }),
            )],
        },
        &BranchState {
            workspace_yaml: "",
            projects: &[(
                ".",
                json!({
                    "name": "v13-peer-merge",
                    "version": "0.0.0",
                    "dependencies": {
                        "@pnpm.e2e/abc-optional-peers": "1.0.0",
                        "@pnpm.e2e/peer-a": "^1.0.1",
                        "@pnpm.e2e/peer-b": "1.0.0",
                        "@pnpm.e2e/qar": "100.0.0",
                    },
                }),
            )],
        },
    );
}

#[test]
fn git_merge_peer_provider_bump_and_new_consumer_produces_valid_lockfile() {
    let ws = "packages:\n  - packages/*\n";
    let root = json!({ "name": "root", "private": true });
    let pkg_a = |peer_a: &str| {
        json!({
            "name": "pkg-a",
            "version": "0.0.0",
            "dependencies": {
                "@pnpm.e2e/abc-optional-peers": "1.0.0",
                "@pnpm.e2e/peer-a": peer_a,
            },
        })
    };
    let pkg_b_empty = json!({ "name": "pkg-b", "version": "0.0.0" });
    let pkg_b_consumer = json!({
        "name": "pkg-b",
        "version": "0.0.0",
        "dependencies": {
            "@pnpm.e2e/abc-optional-peers": "1.0.0",
        },
    });
    assert_clean_v13_git_merge(
        &BranchState {
            workspace_yaml: ws,
            projects: &[
                (".", root.clone()),
                ("packages/a", pkg_a("1.0.0")),
                ("packages/b", pkg_b_empty.clone()),
            ],
        },
        &BranchState {
            workspace_yaml: ws,
            projects: &[
                (".", root.clone()),
                ("packages/a", pkg_a("1.0.1")),
                ("packages/b", pkg_b_empty),
            ],
        },
        &BranchState {
            workspace_yaml: ws,
            projects: &[
                (".", root.clone()),
                ("packages/a", pkg_a("1.0.0")),
                ("packages/b", pkg_b_consumer.clone()),
            ],
        },
        &BranchState {
            workspace_yaml: ws,
            projects: &[
                (".", root),
                ("packages/a", pkg_a("1.0.1")),
                ("packages/b", pkg_b_consumer),
            ],
        },
    );
}

#[test]
fn git_merge_adding_workspace_project_and_bumping_adjacent_project_merges_cleanly() {
    let ws = "packages:\n  - packages/*\n";
    let root = json!({ "name": "root", "private": true });
    let pkg_a = json!({ "name": "pkg-a", "version": "0.0.0" });
    let pkg_b = json!({
        "name": "pkg-b",
        "version": "0.0.0",
        "dependencies": {
            "@pnpm.e2e/qar": "100.0.0",
        },
    });
    let pkg_c = |foo: &str| {
        json!({
            "name": "pkg-c",
            "version": "0.0.0",
            "dependencies": {
                "@pnpm.e2e/foo": foo,
            },
        })
    };
    assert_clean_v13_git_merge(
        &BranchState {
            workspace_yaml: ws,
            projects: &[
                (".", root.clone()),
                ("packages/a", pkg_a.clone()),
                ("packages/c", pkg_c("100.0.0")),
            ],
        },
        &BranchState {
            workspace_yaml: ws,
            projects: &[
                (".", root.clone()),
                ("packages/a", pkg_a.clone()),
                ("packages/b", pkg_b.clone()),
                ("packages/c", pkg_c("100.0.0")),
            ],
        },
        &BranchState {
            workspace_yaml: ws,
            projects: &[
                (".", root.clone()),
                ("packages/a", pkg_a.clone()),
                ("packages/c", pkg_c("100.1.0")),
            ],
        },
        &BranchState {
            workspace_yaml: ws,
            projects: &[
                (".", root),
                ("packages/a", pkg_a),
                ("packages/b", pkg_b),
                ("packages/c", pkg_c("100.1.0")),
            ],
        },
    );
}
