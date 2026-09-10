use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use std::{ffi::OsStr, fs, path::Path, process::Command};
use tempfile::TempDir;

/// Published at 100.0.0, 100.1.0, and 101.0.0.
const DEP: &str = "@pnpm.e2e/dep-of-pkg-with-1-dep";

fn project_manifest(name: &str) -> String {
    serde_json::json!({
        "name": name,
        "version": "1.0.0",
        "dependencies": { DEP: "^100.0.0" },
        "scripts": {
            "postinstall": format!(
                r#"node -e "require('fs').writeFileSync('ran-postinstall.txt','{name}')""#,
            ),
        },
    })
    .to_string()
}

/// A workspace whose root and `packages/*` members all carry a
/// `postinstall` that stamps `ran-postinstall.txt` in their own
/// directory. Returns it already installed, with every stamp the
/// install left behind removed, so a later assertion sees only what
/// the command under test ran.
fn installed_workspace(members: &[&str]) -> (TempDir, std::path::PathBuf, AddMockedRegistry) {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();

    let yaml_path = workspace.join("pnpm-workspace.yaml");
    let yaml = fs::read_to_string(&yaml_path).expect("read pnpm-workspace.yaml");
    fs::write(&yaml_path, format!("{}\npackages:\n  - 'packages/*'\n", yaml.trim_end()))
        .expect("write pnpm-workspace.yaml");
    fs::write(workspace.join("package.json"), project_manifest("root"))
        .expect("write the root package.json");
    for member in members {
        let dir = workspace.join("packages").join(member);
        fs::create_dir_all(&dir).expect("create the member dir");
        fs::write(dir.join("package.json"), project_manifest(member))
            .expect("write the member package.json");
    }

    pacquet(&workspace, ["install"]).assert().success();
    clear_stamps(&workspace, members);

    (root, workspace, npmrc_info)
}

fn pacquet(cwd: &Path, args: impl IntoIterator<Item = impl AsRef<OsStr>>) -> Command {
    Command::cargo_bin("pnpm").expect("find the pnpm binary").with_current_dir(cwd).with_args(args)
}

fn stamp_path(workspace: &Path, project: &str) -> std::path::PathBuf {
    let dir = if project == "root" {
        workspace.to_path_buf()
    } else {
        workspace.join("packages").join(project)
    };
    dir.join("ran-postinstall.txt")
}

fn clear_stamps(workspace: &Path, members: &[&str]) {
    for project in std::iter::once(&"root").chain(members) {
        let path = stamp_path(workspace, project);
        if path.exists() {
            fs::remove_file(path).expect("clear a postinstall stamp");
        }
    }
}

#[track_caller]
fn assert_ran(workspace: &Path, expected: &[&str], all: &[&str]) {
    let ran: Vec<&str> =
        all.iter().copied().filter(|project| stamp_path(workspace, project).exists()).collect();
    assert_eq!(ran, expected, "projects whose own postinstall ran");
}

/// A targeted `update <selector>` is pnpm's `installSome`: the project
/// it was run in does not run its own lifecycle scripts. The workspace
/// root still does — the selection left it out, so it is mutated as a
/// plain full install, and here root plus the lone member is the whole
/// workspace, which is what puts the partial mutation under the
/// `mutation === 'install'` filter.
#[test]
fn targeted_update_in_the_only_member_runs_only_the_workspace_root_scripts() {
    let (root, workspace, anchor) = installed_workspace(&["a"]);

    pacquet(&workspace.join("packages").join("a"), ["update", DEP]).assert().success();

    assert_ran(&workspace, &["root"], &["root", "a"]);

    drop((root, anchor));
}

/// Once a member the command never touched is left over, the mutated
/// set covers only part of the workspace — the case pnpm materializes
/// the rest of the workspace for, and where every mutated project runs
/// its own scripts whatever its mutation. The targeted member runs
/// them too; only the untouched member stays silent.
#[test]
fn targeted_update_in_a_larger_workspace_runs_the_mutated_projects_scripts() {
    let (root, workspace, anchor) = installed_workspace(&["a", "b"]);

    pacquet(&workspace.join("packages").join("a"), ["update", DEP]).assert().success();

    assert_ran(&workspace, &["root", "a"], &["root", "a", "b"]);

    drop((root, anchor));
}

/// A selector-less `update` is a full install of the project it was
/// run in, so that project runs its own scripts — and so does the
/// unselected workspace root. The workspace members the command never
/// touched stay silent.
#[test]
fn bare_update_in_a_member_runs_that_member_and_the_workspace_root() {
    let (root, workspace, anchor) = installed_workspace(&["a", "b"]);

    pacquet(&workspace.join("packages").join("a"), ["update"]).assert().success();

    assert_ran(&workspace, &["root", "a"], &["root", "a", "b"]);

    drop((root, anchor));
}

#[test]
fn filtered_bare_update_runs_the_selected_member_and_workspace_root() {
    let (root, workspace, anchor) = installed_workspace(&["a", "b"]);

    pacquet(&workspace, ["--filter", "a", "update"]).assert().success();

    assert_ran(&workspace, &["root", "a"], &["root", "a", "b"]);

    drop((root, anchor));
}

/// Run at the workspace root, an update mutates the root alone: the
/// members are materialized from the lockfile but run no scripts.
#[test]
fn update_at_the_workspace_root_runs_only_the_root_scripts() {
    let (root, workspace, anchor) = installed_workspace(&["a", "b"]);

    pacquet(&workspace, ["update"]).assert().success();

    assert_ran(&workspace, &["root"], &["root", "a", "b"]);

    drop((root, anchor));
}

/// `-r` mutates every project, so nothing is left for the workspace
/// root to be pushed in for — and a targeted update makes all of those
/// mutations partial. No project runs its own scripts.
#[test]
fn recursive_targeted_update_runs_no_project_scripts() {
    let (root, workspace, anchor) = installed_workspace(&["a", "b"]);

    pacquet(&workspace, ["-r", "update", DEP]).assert().success();

    assert_ran(&workspace, &[], &["root", "a", "b"]);

    drop((root, anchor));
}

/// A recursive selector-less update installs every project in full,
/// so every project runs its own scripts.
#[test]
fn recursive_bare_update_runs_every_project_scripts() {
    let (root, workspace, anchor) = installed_workspace(&["a", "b"]);

    pacquet(&workspace, ["-r", "update"]).assert().success();

    assert_ran(&workspace, &["root", "a", "b"], &["root", "a", "b"]);

    drop((root, anchor));
}

/// `add` is `installSome` like a targeted update, and follows the same
/// rule: in a workspace it leaves the rest of the members untouched
/// while the root — mutated as a full install — runs its scripts. The
/// member the `add` ran in runs its own scripts too, because the
/// mutated set covers only part of the workspace.
#[test]
fn add_in_a_member_runs_that_member_and_the_workspace_root() {
    let (root, workspace, anchor) = installed_workspace(&["a", "b"]);

    pacquet(&workspace.join("packages").join("a"), ["add", "@pnpm.e2e/foo"]).assert().success();

    assert_ran(&workspace, &["root", "a"], &["root", "a", "b"]);

    drop((root, anchor));
}

/// `remove` is pnpm's `uninstallSome`, which runs no project's own
/// lifecycle scripts anywhere in the workspace.
#[test]
fn remove_in_a_member_runs_no_project_scripts() {
    let (root, workspace, anchor) = installed_workspace(&["a", "b"]);

    pacquet(&workspace.join("packages").join("a"), ["remove", DEP]).assert().success();

    assert_ran(&workspace, &[], &["root", "a", "b"]);

    drop((root, anchor));
}
