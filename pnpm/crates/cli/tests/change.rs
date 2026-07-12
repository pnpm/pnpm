//! Integration tests for `pnpm change` and the intent-consuming
//! `pnpm version`: recording an intent, printing the pending release plan,
//! applying it (manifest bumps, changelogs, the consumed-intents ledger,
//! intent-file cleanup), snapshot releases, and release-lane management via
//! `pnpm lane`. Mirrors the
//! TypeScript CLI's `pnpm11/releasing/commands/test/change/index.test.ts`.

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pacquet_testing_utils::bin::CommandTempCwd;
use std::{fs, path::Path, process::Command};

fn write_workspace(workspace: &Path) {
    fs::write(workspace.join("pnpm-workspace.yaml"), "packages:\n  - packages/*\n")
        .expect("write pnpm-workspace.yaml");
    fs::write(workspace.join("package.json"), "{\"name\": \"e2e-root\", \"private\": true}\n")
        .expect("write root package.json");
}

fn add_pkg(workspace: &Path, name: &str, version: &str, deps: &str) {
    let dir = workspace.join("packages").join(name);
    fs::create_dir_all(&dir).expect("create package dir");
    fs::write(
        dir.join("package.json"),
        format!("{{\"name\": \"{name}\", \"version\": \"{version}\", \"dependencies\": {deps}}}\n"),
    )
    .expect("write package.json");
}

fn pnpm(workspace: &Path) -> Command {
    Command::cargo_bin("pnpm").expect("find the pnpm binary").with_current_dir(workspace)
}

fn stdout_of(mut command: Command) -> String {
    let output = command.output().expect("run pnpm");
    assert!(
        output.status.success(),
        "command failed.\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn manifest_version(workspace: &Path, name: &str) -> String {
    let manifest: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(workspace.join("packages").join(name).join("package.json"))
            .expect("read package.json"),
    )
    .expect("parse package.json");
    manifest["version"].as_str().expect("version is a string").to_string()
}

#[test]
fn change_records_an_intent_and_version_applies_the_release_plan() {
    let CommandTempCwd { workspace, root, .. } = CommandTempCwd::init();
    write_workspace(&workspace);
    add_pkg(&workspace, "lib", "1.2.0", "{}");
    add_pkg(&workspace, "cli", "3.0.0", r#"{"lib": "workspace:^"}"#);

    let output = stdout_of(pnpm(&workspace).with_args([
        "change",
        "--bump",
        "major",
        "--summary",
        "Rewrote the widget API.",
        "lib",
    ]));
    assert!(output.contains("Recorded change intent .changeset/"), "unexpected: {output}");

    let status = stdout_of(pnpm(&workspace).with_args(["change", "status"]));
    assert!(status.contains("lib: 1.2.0 → 2.0.0 (major, via intent)"), "unexpected: {status}");
    assert!(
        status.contains("cli: 3.0.0 → 3.0.1 (patch, via dependencies)"),
        "unexpected: {status}",
    );

    let dry_run = stdout_of(pnpm(&workspace).with_args(["version", "-r", "--dry-run"]));
    assert!(dry_run.contains("lib: 1.2.0 → 2.0.0"), "unexpected: {dry_run}");

    let applied = stdout_of(pnpm(&workspace).with_args(["version", "-r"]));
    assert!(applied.contains("lib: 1.2.0 → 2.0.0"), "unexpected: {applied}");
    assert!(applied.contains("cli: 3.0.0 → 3.0.1"), "unexpected: {applied}");

    assert_eq!(manifest_version(&workspace, "lib"), "2.0.0");
    assert_eq!(manifest_version(&workspace, "cli"), "3.0.1");

    let lib_changelog =
        fs::read_to_string(workspace.join("packages/lib/CHANGELOG.md")).expect("read changelog");
    assert!(lib_changelog.contains("## 2.0.0"), "unexpected: {lib_changelog}");
    assert!(lib_changelog.contains("- Rewrote the widget API."), "unexpected: {lib_changelog}");
    let cli_changelog =
        fs::read_to_string(workspace.join("packages/cli/CHANGELOG.md")).expect("read changelog");
    assert!(cli_changelog.contains("  - lib@2.0.0"), "unexpected: {cli_changelog}");

    let ledger = fs::read_to_string(workspace.join(".changeset/ledger.yaml")).expect("read ledger");
    assert!(ledger.contains("lib@2.0.0:"), "unexpected: {ledger}");

    let leftover_intents: Vec<_> = fs::read_dir(workspace.join(".changeset"))
        .expect("read .changeset")
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().ends_with(".md"))
        .collect();
    assert!(leftover_intents.is_empty(), "intent files were not cleaned up");

    let no_pending = stdout_of(pnpm(&workspace).with_args(["version", "-r"]));
    assert!(no_pending.contains("No pending changes"), "unexpected: {no_pending}");

    drop(root);
}

#[test]
fn lanes_are_entered_released_and_graduated() {
    let CommandTempCwd { workspace, root, .. } = CommandTempCwd::init();
    write_workspace(&workspace);
    add_pkg(&workspace, "cli", "2.0.0", "{}");

    let bare = stdout_of(pnpm(&workspace).with_args(["lane"]));
    assert!(bare.contains("All packages are on the main lane."), "unexpected: {bare}");

    let entered = stdout_of(pnpm(&workspace).with_args(["lane", "alpha", "--filter", "cli"]));
    assert!(entered.contains(r#"Moved to the "alpha" lane:"#), "unexpected: {entered}");

    let membership = stdout_of(pnpm(&workspace).with_args(["lane"]));
    assert!(membership.contains("alpha:"), "unexpected: {membership}");
    assert!(membership.contains("    cli"), "unexpected: {membership}");
    let manifest = fs::read_to_string(workspace.join("pnpm-workspace.yaml")).expect("read yaml");
    assert!(manifest.contains("cli: alpha"), "unexpected: {manifest}");

    stdout_of(pnpm(&workspace).with_args([
        "change",
        "--bump",
        "minor",
        "--summary",
        "Added a flag.",
        "cli",
    ]));
    let applied = stdout_of(pnpm(&workspace).with_args(["version", "-r"]));
    assert!(applied.contains("cli: 2.0.0 → 2.1.0-alpha.0"), "unexpected: {applied}");

    // The intent survives the prerelease: its prose is needed at graduation.
    let intents: Vec<_> = fs::read_dir(workspace.join(".changeset"))
        .expect("read .changeset")
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().ends_with(".md"))
        .collect();
    assert_eq!(intents.len(), 1, "the intent must survive until graduation");

    let exited = stdout_of(pnpm(&workspace).with_args(["lane", "main", "--filter", "cli"]));
    assert!(exited.contains("Moved to the main lane:"), "unexpected: {exited}");

    let graduated = stdout_of(pnpm(&workspace).with_args(["version", "-r"]));
    assert!(graduated.contains("cli: 2.1.0-alpha.0 → 2.1.0"), "unexpected: {graduated}");

    let changelog =
        fs::read_to_string(workspace.join("packages/cli/CHANGELOG.md")).expect("read changelog");
    assert!(changelog.contains("## 2.1.0-alpha.0"), "unexpected: {changelog}");
    assert!(changelog.contains("## 2.1.0"), "unexpected: {changelog}");

    let manifest = fs::read_to_string(workspace.join("pnpm-workspace.yaml")).expect("read yaml");
    assert!(!manifest.contains("alpha"), "the versioning key must be cleaned up: {manifest}");

    drop(root);
}

#[test]
fn snapshot_releases_do_not_consume_intents() {
    let CommandTempCwd { workspace, root, .. } = CommandTempCwd::init();
    write_workspace(&workspace);
    add_pkg(&workspace, "lib", "1.0.0", "{}");

    stdout_of(pnpm(&workspace).with_args([
        "change",
        "--bump",
        "patch",
        "--summary",
        "A fix.",
        "lib",
    ]));
    let applied = stdout_of(pnpm(&workspace).with_args(["version", "-r", "--snapshot", "preview"]));
    assert!(applied.contains("lib: 1.0.0 → 0.0.0-preview-"), "unexpected: {applied}");

    let intents: Vec<_> = fs::read_dir(workspace.join(".changeset"))
        .expect("read .changeset")
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().ends_with(".md"))
        .collect();
    assert_eq!(intents.len(), 1, "snapshot releases must not consume intents");
    assert!(!workspace.join("packages/lib/CHANGELOG.md").exists());

    drop(root);
}

#[test]
fn version_without_arguments_outside_recursive_mode_requires_a_bump() {
    let CommandTempCwd { workspace, root, .. } = CommandTempCwd::init();
    write_workspace(&workspace);
    add_pkg(&workspace, "lib", "1.0.0", "{}");

    let output = pnpm(&workspace).with_arg("version").output().expect("run pnpm");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("A version argument is required"), "unexpected: {stderr}");

    drop(root);
}

#[test]
fn lane_assignment_requires_a_filter() {
    let CommandTempCwd { workspace, root, .. } = CommandTempCwd::init();
    write_workspace(&workspace);
    add_pkg(&workspace, "cli", "2.0.0", "{}");

    let output = pnpm(&workspace).with_args(["lane", "alpha"]).output().expect("run pnpm");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("--filter"), "unexpected: {stderr}");

    drop(root);
}
