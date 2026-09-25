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
    Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(cwd)
        .with_args(args)
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
    let ran: Vec<&str> = all
        .iter()
        .copied()
        .filter(|project| stamp_path(workspace, project).exists())
        .collect();
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

#[test]
fn add_in_a_member_does_not_run_prepare_scripts() {
    let (root, workspace, anchor) = installed_workspace(&["a", "b"]);

    let pkg_a_dir = workspace.join("packages").join("a");
    let mut manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(pkg_a_dir.join("package.json")).unwrap()).unwrap();
    manifest["scripts"]["prepare"] =
        serde_json::json!(r#"node -e "require('fs').writeFileSync('ran-prepare.txt','a')""#);
    fs::write(pkg_a_dir.join("package.json"), manifest.to_string()).unwrap();

    pacquet(&pkg_a_dir, ["add", "@pnpm.e2e/foo"]).assert().success();

    assert_ran(&workspace, &["root", "a"], &["root", "a", "b"]);
    assert!(
        !pkg_a_dir.join("ran-prepare.txt").exists(),
        "prepare script must not run during add in workspace member",
    );

    drop((root, anchor));
}

#[test]
fn add_in_a_member_saves_manifest_when_root_postinstall_fails() {
    let (root, workspace, anchor) = installed_workspace(&["a"]);

    let root_pkg_json = workspace.join("package.json");
    let mut root_manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&root_pkg_json).unwrap()).unwrap();
    root_manifest["scripts"]["postinstall"] = serde_json::json!("exit 1");
    fs::write(&root_pkg_json, root_manifest.to_string()).unwrap();

    let pkg_a_dir = workspace.join("packages").join("a");
    pacquet(&workspace, ["--filter", "a", "add", "@pnpm.e2e/foo"]).assert().failure();

    let a_manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(pkg_a_dir.join("package.json")).unwrap()).unwrap();
    assert!(
        a_manifest["dependencies"]["@pnpm.e2e/foo"].is_string(),
        "package.json must be saved even if postinstall fails",
    );

    drop((root, anchor));
}

#[test]
fn add_in_a_member_saves_manifest_when_root_postinstall_fails_without_filter() {
    let (root, workspace, anchor) = installed_workspace(&["a"]);

    let root_pkg_json = workspace.join("package.json");
    let mut root_manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&root_pkg_json).unwrap()).unwrap();
    root_manifest["scripts"]["postinstall"] = serde_json::json!("exit 1");
    fs::write(&root_pkg_json, root_manifest.to_string()).unwrap();

    let pkg_a_dir = workspace.join("packages").join("a");
    pacquet(&pkg_a_dir, ["add", "@pnpm.e2e/foo"]).assert().failure();

    let a_manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(pkg_a_dir.join("package.json")).unwrap()).unwrap();
    assert!(
        a_manifest["dependencies"]["@pnpm.e2e/foo"].is_string(),
        "package.json must be saved even if postinstall fails",
    );

    drop((root, anchor));
}

#[test]
fn add_in_a_member_does_not_save_manifest_when_preinstall_fails() {
    let (root, workspace, anchor) = installed_workspace(&["a"]);

    let root_pkg_json = workspace.join("package.json");
    let mut root_manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&root_pkg_json).unwrap()).unwrap();
    root_manifest["scripts"]["preinstall"] = serde_json::json!("exit 1");
    fs::write(&root_pkg_json, root_manifest.to_string()).unwrap();

    let pkg_a_dir = workspace.join("packages").join("a");
    pacquet(&pkg_a_dir, ["add", "@pnpm.e2e/foo"]).assert().failure();

    let a_manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(pkg_a_dir.join("package.json")).unwrap()).unwrap();
    assert!(
        a_manifest
            .get("dependencies")
            .and_then(|deps| deps.get("@pnpm.e2e/foo"))
            .is_none(),
        "package.json must not be saved when preinstall fails",
    );

    drop((root, anchor));
}

/// A `prepare` that appends `name` to `order.txt` in the workspace
/// root (`INIT_CWD`) after `delay_ms`.
fn append_order_after(name: &str, delay_ms: u32) -> String {
    format!(
        r#"node -e "setTimeout(() => require('fs').appendFileSync(process.env.INIT_CWD + '/order.txt', '{name}\n'), {delay_ms})""#,
    )
}

/// The dependency's script sleeps and the dependent's does not, so
/// `order.txt` records the sequencing rather than the script
/// durations: without the edge `app` would finish first.
#[test]
fn override_to_a_workspace_sibling_orders_the_project_scripts() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();

    let yaml_path = workspace.join("pnpm-workspace.yaml");
    let yaml = fs::read_to_string(&yaml_path).expect("read pnpm-workspace.yaml");
    fs::write(
        &yaml_path,
        format!(
            "{}\npackages:\n  - 'packages/*'\noverrides:\n  lib: 'workspace:*'\n",
            yaml.trim_end(),
        ),
    )
    .expect("write pnpm-workspace.yaml");
    fs::write(workspace.join("package.json"), r#"{ "name": "root" }"#)
        .expect("write the root package.json");
    for (dir, manifest) in [
        (
            "lib",
            serde_json::json!({
                "name": "lib",
                "version": "1.0.0",
                "scripts": { "prepare": append_order_after("lib", 500) },
            }),
        ),
        (
            "app",
            serde_json::json!({
                "name": "app",
                "version": "1.0.0",
                "dependencies": { "lib": "^1.0.0" },
                "scripts": { "prepare": append_order_after("app", 0) },
            }),
        ),
    ] {
        let dir = workspace.join("packages").join(dir);
        fs::create_dir_all(&dir).expect("create the member dir");
        fs::write(dir.join("package.json"), manifest.to_string())
            .expect("write the member package.json");
    }

    pacquet(&workspace, ["install"]).assert().success();

    let order = fs::read_to_string(workspace.join("order.txt")).expect("read order.txt");
    assert_eq!(order.lines().collect::<Vec<_>>(), ["lib", "app"]);

    drop((root, npmrc_info));
}
