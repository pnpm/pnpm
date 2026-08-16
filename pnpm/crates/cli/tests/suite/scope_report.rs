//! Coverage for the `Scope:` line: which workspace projects a command
//! reports it selected, and which commands say nothing at all.

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use std::{fs, path::Path, process::Command};

fn pacquet(workspace: &Path) -> Command {
    Command::cargo_bin("pnpm").expect("find the pnpm binary").with_current_dir(workspace)
}

/// A three-project workspace (root plus two members) with one mocked
/// dependency on the root, installed enough to have a lockfile.
fn three_project_workspace(root_manifest: &serde_json::Value) -> CommandTempCwd<AddMockedRegistry> {
    let fixture = CommandTempCwd::init().add_mocked_registry();
    fs::write(fixture.workspace.join("package.json"), root_manifest.to_string())
        .expect("write root package.json");

    let workspace_yaml_path = fixture.workspace.join("pnpm-workspace.yaml");
    let mut workspace_yaml =
        fs::read_to_string(&workspace_yaml_path).expect("read pnpm-workspace.yaml");
    if !workspace_yaml.ends_with('\n') {
        workspace_yaml.push('\n');
    }
    workspace_yaml.push_str("packages:\n  - 'pkg-*'\n");
    fs::write(&workspace_yaml_path, workspace_yaml).expect("write pnpm-workspace.yaml");

    for name in ["pkg-a", "pkg-b"] {
        fs::create_dir(fixture.workspace.join(name)).expect("mkdir member");
        fs::write(
            fixture.workspace.join(name).join("package.json"),
            serde_json::json!({ "name": name, "version": "1.0.0", "private": true }).to_string(),
        )
        .expect("write member package.json");
    }
    fixture
}

fn default_workspace() -> CommandTempCwd<AddMockedRegistry> {
    three_project_workspace(&serde_json::json!({
        "name": "root",
        "version": "1.0.0",
        "private": true,
        "dependencies": { "@pnpm.e2e/hello-world-js-bin": "1.0.0" },
    }))
}

/// Everything the command printed. The reporter writes to stdout and
/// diagnostics to stderr, and which stream a line lands on is not what
/// these tests are about.
/// The workspace path as the CLI reports it: `--dir` is canonicalized
/// before anything is emitted, so on macOS the reported prefix resolves
/// `/var` to `/private/var` while the `TempDir` path does not.
fn reported_prefix(workspace: &Path) -> String {
    dunce::canonicalize(workspace).expect("canonicalize workspace").to_string_lossy().into_owned()
}

/// The `pnpm:scope` records in an NDJSON run, decoded.
fn scope_records(printed: &str) -> Vec<serde_json::Value> {
    printed
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter(|record| record["name"] == "pnpm:scope")
        .collect()
}

fn output_of(mut command: Command) -> String {
    let output = command.output().expect("run pnpm");
    let printed = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    assert!(output.status.success(), "command failed\noutput:\n{printed}");
    printed
}

#[test]
fn install_reports_the_whole_workspace_as_its_scope() {
    let CommandTempCwd { workspace, root, npmrc_info, .. } = default_workspace();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let printed = output_of(pacquet(&workspace).with_arg("install"));
    assert!(printed.contains("Scope: all 3 workspace projects"), "output:\n{printed}");

    // The repeat-install short-circuit is the common case, and it covers
    // the same projects — it reports the scope rather than going quiet
    // about what it just decided was current.
    let printed = output_of(pacquet(&workspace).with_arg("install"));
    assert!(printed.contains("Already up to date"), "the short-circuit ran:\n{printed}");
    assert!(printed.contains("Scope: all 3 workspace projects"), "output:\n{printed}");

    drop((mock_instance, root));
}

#[test]
fn a_filtered_install_reports_how_much_of_the_workspace_it_selected() {
    let CommandTempCwd { workspace, root, npmrc_info, .. } = default_workspace();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    pacquet(&workspace).with_arg("install").assert().success();
    let printed = output_of(pacquet(&workspace).with_args(["install", "--filter", "pkg-*"]));
    assert!(printed.contains("Scope: 2 of 3 workspace projects"), "output:\n{printed}");

    drop((mock_instance, root));
}

/// A single selected project is the directory the user is standing in,
/// and `add` is not one of the commands pnpm reports scope for — neither
/// says anything.
#[test]
fn a_single_project_selection_and_a_non_reporting_command_stay_silent() {
    let CommandTempCwd { workspace, root, npmrc_info, .. } = default_workspace();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    pacquet(&workspace).with_arg("install").assert().success();

    let printed = output_of(pacquet(&workspace).with_args(["install", "--filter", "pkg-a"]));
    assert!(!printed.contains("Scope:"), "one selected project: {printed}");

    let printed = output_of(pacquet(&workspace).with_args(["add", "@pnpm.e2e/hello-world-js-bin"]));
    assert!(!printed.contains("Scope:"), "add does not report scope: {printed}");

    drop((mock_instance, root));
}

/// `run -r` reports the same scope the install family does, and the
/// workspace-root auto-exclusion is part of what it counts as selected.
#[test]
fn a_recursive_run_reports_the_scope_it_selected() {
    let CommandTempCwd { workspace, root, npmrc_info, .. } = default_workspace();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    for member in ["pkg-a", "pkg-b"] {
        fs::write(
            workspace.join(member).join("package.json"),
            serde_json::json!({
                "name": member,
                "version": "1.0.0",
                "private": true,
                "scripts": { "greet": r#"node -e "console.log('hi')""# },
            })
            .to_string(),
        )
        .expect("write member package.json");
    }
    pacquet(&workspace).with_arg("install").assert().success();

    // The workspace root is auto-excluded from a recursive `run`, so two
    // of the three projects are selected.
    let printed = output_of(pacquet(&workspace).with_args(["-r", "run", "greet"]));
    assert!(printed.contains("Scope: 2 of 3 workspace projects"), "output:\n{printed}");

    let printed = output_of(pacquet(&workspace).with_args(["--filter", "pkg-a", "run", "greet"]));
    assert!(!printed.contains("Scope:"), "one selected project: {printed}");

    drop((mock_instance, root));
}

/// With `sharedWorkspaceLockfile: false` a filtered install runs each
/// selected project separately. The scope is resolved once, so it must be
/// reported once — a child install reporting the whole workspace would
/// both duplicate the record and overwrite the filtered count.
#[test]
fn a_dedicated_lockfile_install_reports_its_scope_once() {
    let CommandTempCwd { workspace, root, npmrc_info, .. } = default_workspace();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let yaml_path = workspace.join("pnpm-workspace.yaml");
    let mut yaml = fs::read_to_string(&yaml_path).expect("read pnpm-workspace.yaml");
    yaml.push_str("sharedWorkspaceLockfile: false\n");
    fs::write(&yaml_path, yaml).expect("write pnpm-workspace.yaml");
    pacquet(&workspace).with_arg("install").assert().success();

    let printed = output_of(pacquet(&workspace).with_args([
        "install",
        "--filter",
        "pkg-*",
        "--reporter=ndjson",
    ]));
    let scopes = scope_records(&printed);
    assert_eq!(scopes.len(), 1, "exactly one scope record: {printed}");
    assert_eq!(scopes[0]["level"], "debug");
    assert_eq!(scopes[0]["selected"], 2);
    assert_eq!(scopes[0]["total"], 3);
    assert_eq!(scopes[0]["workspacePrefix"], reported_prefix(&workspace));

    drop((mock_instance, root));
}

/// pnpm reports the workspace only for a run that covers it. A partial
/// install targets the project it was run in, and says so with the
/// single-project payload — `selected: 1` and no `total` — which is also
/// what stops the reporter rendering a `Scope:` line for it.
#[test]
fn a_partial_install_reports_the_single_project_shape() {
    let CommandTempCwd { workspace, root, npmrc_info, .. } = default_workspace();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    pacquet(&workspace).with_arg("install").assert().success();
    let printed = output_of(pacquet(&workspace).with_args([
        "add",
        "@pnpm.e2e/hello-world-js-bin",
        "--reporter=ndjson",
    ]));

    let scopes = scope_records(&printed);
    assert_eq!(scopes.len(), 1, "exactly one scope record: {printed}");
    assert_eq!(scopes[0]["selected"], 1);
    assert!(scopes[0].get("total").is_none(), "no total: {}", scopes[0]);
    assert_eq!(scopes[0]["workspacePrefix"], reported_prefix(&workspace));

    drop((mock_instance, root));
}
