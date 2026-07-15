use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pacquet_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use std::{ffi::OsStr, fs, path::Path, process::Command};
use tempfile::TempDir;

const DEP: &str = "@pnpm.e2e/dep-of-pkg-with-1-dep";
const HELLO: &str = "@pnpm.e2e/hello-world-js-bin";
const PKG: &str = "@pnpm.e2e/pkg-with-1-dep";

fn setup() -> (TempDir, std::path::PathBuf, AddMockedRegistry) {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    (root, workspace, npmrc_info)
}

fn pacquet(workspace: &Path, args: impl IntoIterator<Item = impl AsRef<OsStr>>) -> Command {
    Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(workspace)
        .with_args(args)
}

fn write_manifest(workspace: &Path, dependencies: &str) {
    let manifest =
        format!(r#"{{ "name": "test-why", "version": "1.0.0", "dependencies": {dependencies} }}"#);
    fs::write(workspace.join("package.json"), manifest).expect("write package.json");
}

#[test]
fn why_fails_without_package_name() {
    let (_root, workspace, _anchor) = setup();

    write_manifest(&workspace, &format!(r#"{{ "{PKG}": "100.0.0" }}"#));
    pacquet(&workspace, ["install"]).assert().success();

    let output = pacquet(&workspace, ["why"]).output().expect("run pacquet why");
    assert!(!output.status.success(), "why without args should fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("requires a package name"),
        "should show error about missing package name: {stderr}",
    );
}

#[test]
fn filtered_why_rejects_per_project_workspace_lockfiles() {
    let (_root, workspace, _anchor) = setup();
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "packages:\n  - packages/*\nsharedWorkspaceLockfile: false\n",
    )
    .expect("write workspace manifest");
    write_manifest(&workspace, "{}");

    let output = pacquet(&workspace, ["--filter", ".", "why", PKG])
        .output()
        .expect("run filtered pacquet why");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("ERR_PNPM_RECURSIVE_SHARED_LOCKFILE_UNSUPPORTED")
            && stderr.contains("sharedWorkspaceLockfile=false"),
        "stderr: {stderr}",
    );
}

#[test]
fn why_shows_reverse_tree_for_direct_dep() {
    let (_root, workspace, _anchor) = setup();

    write_manifest(&workspace, &format!(r#"{{ "{PKG}": "100.0.0" }}"#));
    pacquet(&workspace, ["install"]).assert().success();

    let output = pacquet(&workspace, ["why", PKG]).output().expect("run pacquet why");
    assert!(output.status.success(), "why should succeed");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains(PKG), "should mention the package: {stdout}");
    assert!(stdout.contains("100.0.0"), "should show the version: {stdout}");
    assert!(stdout.contains("test-why"), "should show the project as a dependent: {stdout}");
}

#[test]
fn why_shows_reverse_tree_for_transitive_dep() {
    let (_root, workspace, _anchor) = setup();

    write_manifest(&workspace, &format!(r#"{{ "{PKG}": "100.0.0" }}"#));
    pacquet(&workspace, ["install"]).assert().success();

    let output = pacquet(&workspace, ["why", DEP]).output().expect("run pacquet why");
    assert!(output.status.success(), "why should succeed");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains(DEP), "should mention the package: {stdout}");
    assert!(stdout.contains(PKG), "should show PKG as a dependent: {stdout}");
    assert!(stdout.contains("test-why"), "should show the project as a dependent: {stdout}");
}

#[test]
fn why_with_glob_pattern() {
    let (_root, workspace, _anchor) = setup();

    write_manifest(&workspace, &format!(r#"{{ "{PKG}": "100.0.0", "{DEP}": "100.0.0" }}"#));
    pacquet(&workspace, ["install"]).assert().success();

    let output = pacquet(&workspace, ["why", "@pnpm.e2e/*"]).output().expect("run pacquet why");
    assert!(output.status.success(), "why with glob should succeed");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains(PKG), "should mention pkg-with-1-dep: {stdout}");
    assert!(stdout.contains(DEP), "should mention dep-of-pkg-with-1-dep: {stdout}");
}

#[test]
fn why_without_lockfile_returns_empty() {
    let (_root, workspace, _anchor) = setup();

    write_manifest(&workspace, &format!(r#"{{ "{PKG}": "100.0.0" }}"#));

    let output = pacquet(&workspace, ["why", PKG]).output().expect("run pacquet why");
    assert!(output.status.success(), "why without lockfile should succeed like pnpm: {output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.is_empty(), "should produce no output without lockfile: {stdout}");
}

#[test]
fn why_depth_limits_output() {
    let (_root, workspace, _anchor) = setup();

    write_manifest(&workspace, &format!(r#"{{ "{PKG}": "100.0.0" }}"#));
    pacquet(&workspace, ["install"]).assert().success();

    let output_full =
        pacquet(&workspace, ["why", DEP]).output().expect("run pacquet why --depth unset");
    let output_depth1 = pacquet(&workspace, ["why", DEP, "--depth", "1"])
        .output()
        .expect("run pacquet why --depth 1");

    let full_stdout = String::from_utf8_lossy(&output_full.stdout);
    let depth1_stdout = String::from_utf8_lossy(&output_depth1.stdout);
    assert!(full_stdout.contains("test-why"), "full output shows project: {full_stdout}");
    assert!(depth1_stdout.contains(DEP), "depth=1 output still shows the target: {depth1_stdout}");
    assert!(depth1_stdout.contains(PKG), "depth=1 output shows direct parent: {depth1_stdout}");
}

#[test]
fn why_from_workspace_member_stays_within_forward_workspace_link_closure() {
    let (_root, workspace, _anchor) = setup();
    fs::write(workspace.join("pnpm-workspace.yaml"), "packages:\n  - packages/*\n")
        .expect("write workspace manifest");
    write_manifest(&workspace, "{}");

    let app = workspace.join("packages/app");
    let linked = workspace.join("packages/linked");
    let sibling = workspace.join("packages/sibling");
    for project in [&app, &linked, &sibling] {
        fs::create_dir_all(project).expect("create workspace project");
    }
    fs::write(
        app.join("package.json"),
        r#"{ "name": "app", "version": "1.0.0", "dependencies": { "linked": "workspace:*" } }"#,
    )
    .expect("write app package.json");
    fs::write(
        linked.join("package.json"),
        format!(
            r#"{{ "name": "linked", "version": "1.0.0", "dependencies": {{ "{PKG}": "100.0.0" }} }}"#,
        ),
    )
    .expect("write linked package.json");
    fs::write(
        sibling.join("package.json"),
        format!(
            r#"{{ "name": "sibling", "version": "1.0.0", "dependencies": {{ "{HELLO}": "1.0.0" }} }}"#,
        ),
    )
    .expect("write sibling package.json");
    pacquet(&workspace, ["install"]).assert().success();

    let sibling_output = pacquet(&app, ["why", HELLO]).output().expect("query sibling dependency");
    assert!(sibling_output.status.success(), "why should succeed: {sibling_output:?}");
    let sibling_stdout = String::from_utf8_lossy(&sibling_output.stdout);
    assert!(
        sibling_stdout.is_empty(),
        "a dependency reachable only from a sibling must not be reported: {sibling_stdout}",
    );

    let linked_output = pacquet(&app, ["why", PKG]).output().expect("query linked dependency");
    assert!(linked_output.status.success(), "why should succeed: {linked_output:?}");
    let linked_stdout = String::from_utf8_lossy(&linked_output.stdout);
    assert!(linked_stdout.contains(PKG), "linked dependency should be reported: {linked_stdout}");
    assert!(
        linked_stdout.contains("packages/linked"),
        "the forward workspace-link closure should be retained: {linked_stdout}",
    );
}

#[test]
fn filtered_why_excludes_unselected_workspace_siblings() {
    let (_root, workspace, _anchor) = setup();
    fs::write(workspace.join("pnpm-workspace.yaml"), "packages:\n  - packages/*\n")
        .expect("write workspace manifest");
    write_manifest(&workspace, "{}");

    let app = workspace.join("packages/app");
    let sibling = workspace.join("packages/sibling");
    fs::create_dir_all(&app).expect("create app project");
    fs::create_dir_all(&sibling).expect("create sibling project");
    fs::write(
        app.join("package.json"),
        format!(
            r#"{{ "name": "app", "version": "1.0.0", "dependencies": {{ "{PKG}": "100.0.0" }} }}"#,
        ),
    )
    .expect("write app package.json");
    fs::write(
        sibling.join("package.json"),
        format!(
            r#"{{ "name": "sibling", "version": "1.0.0", "dependencies": {{ "{HELLO}": "1.0.0" }} }}"#,
        ),
    )
    .expect("write sibling package.json");
    pacquet(&workspace, ["install"]).assert().success();

    let excluded = pacquet(&workspace, ["--filter", "app", "why", HELLO])
        .output()
        .expect("query unselected sibling dependency");
    assert!(excluded.status.success(), "filtered why should succeed: {excluded:?}");
    assert!(
        String::from_utf8_lossy(&excluded.stdout).is_empty(),
        "a dependency reachable only from an unselected sibling must not be reported: {}",
        String::from_utf8_lossy(&excluded.stdout),
    );

    let included = pacquet(&workspace, ["--filter", "app", "why", PKG])
        .output()
        .expect("query selected dependency");
    assert!(included.status.success(), "filtered why should succeed: {included:?}");
    assert!(
        String::from_utf8_lossy(&included.stdout).contains(PKG),
        "a selected dependency should be reported: {}",
        String::from_utf8_lossy(&included.stdout),
    );
}

#[test]
fn recursive_why_includes_all_workspace_projects() {
    let (_root, workspace, _anchor) = setup();
    fs::write(workspace.join("pnpm-workspace.yaml"), "packages:\n  - packages/*\n")
        .expect("write workspace manifest");
    write_manifest(&workspace, "{}");
    let sibling = workspace.join("packages/sibling");
    fs::create_dir_all(&sibling).expect("create workspace project");
    fs::write(
        sibling.join("package.json"),
        format!(
            r#"{{ "name": "sibling", "version": "1.0.0", "dependencies": {{ "{HELLO}": "1.0.0" }} }}"#,
        ),
    )
    .expect("write sibling package.json");
    pacquet(&workspace, ["install"]).assert().success();

    let output = pacquet(&workspace, ["-r", "why", HELLO])
        .output()
        .expect("query recursive workspace dependency");

    assert!(output.status.success(), "recursive why should succeed: {output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains(HELLO), "recursive why should include sibling dependencies: {stdout}");
    assert!(
        stdout.contains("packages/sibling"),
        "recursive why should include the sibling importer: {stdout}",
    );
}
