//! Integration tests for `pnpm change` and the intent-consuming
//! `pnpm version`: recording an intent, printing the pending release plan,
//! applying it (manifest bumps, changelogs, the consumed-intents ledger,
//! intent-file cleanup) and release-lane management via
//! `pnpm lane`. Mirrors the
//! TypeScript CLI's `pnpm11/releasing/commands/test/change/index.test.ts`.

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pacquet_testing_utils::bin::CommandTempCwd;
use std::{fs, path::Path, process::Command};

/// A `pnpm` command that actually probes the registry (no assume-published
/// seam), for the first-release tests that run against the mock registry.
fn pnpm_probing(workspace: &Path) -> Command {
    Command::cargo_bin("pnpm").expect("find the pnpm binary").with_current_dir(workspace)
}

/// Point a mocked-registry workspace at `packages/*` and give it a private
/// root; `add_mocked_registry` writes store/cache config but no package globs.
fn setup_mock_workspace(workspace: &Path) {
    let mut yaml = fs::read_to_string(workspace.join("pnpm-workspace.yaml")).expect("read yaml");
    yaml.push_str("packages:\n  - packages/*\n");
    fs::write(workspace.join("pnpm-workspace.yaml"), yaml).expect("write yaml");
    fs::write(workspace.join("package.json"), "{\"name\": \"e2e-root\", \"private\": true}\n")
        .expect("write root package.json");
}

fn add_scoped_pkg(workspace: &Path, dir: &str, name: &str, version: &str) {
    let pkg_dir = workspace.join("packages").join(dir);
    fs::create_dir_all(&pkg_dir).expect("create package dir");
    fs::write(
        pkg_dir.join("package.json"),
        format!("{{\"name\": \"{name}\", \"version\": \"{version}\"}}\n"),
    )
    .expect("write package.json");
}

/// The real registry probe (no seam): `@pnpm.e2e/foo@1.2.0` is in the fixture
/// packument, so it bumps as a follow-up release.
#[test]
fn first_release_probe_bumps_a_version_the_registry_reports_published() {
    let CommandTempCwd { workspace, root, .. } = CommandTempCwd::init().add_mocked_registry();
    setup_mock_workspace(&workspace);
    add_scoped_pkg(&workspace, "foo", "@pnpm.e2e/foo", "1.2.0");

    stdout_of(pnpm_probing(&workspace).with_args([
        "change",
        "--bump",
        "minor",
        "--summary",
        "A feature.",
        "@pnpm.e2e/foo",
    ]));
    let applied = stdout_of(pnpm_probing(&workspace).with_args(["version", "-r"]));
    assert!(applied.contains("@pnpm.e2e/foo: 1.2.0 → 1.3.0"), "unexpected: {applied}");
    assert_eq!(manifest_version(&workspace, "foo"), "1.3.0");

    drop(root);
}

/// The mirror: `@pnpm.e2e/foo@999.0.0` is not in the fixture, so the probe
/// reports it unpublished and it debuts verbatim.
#[test]
fn first_release_probe_debuts_an_unpublished_version_verbatim() {
    let CommandTempCwd { workspace, root, .. } = CommandTempCwd::init().add_mocked_registry();
    setup_mock_workspace(&workspace);
    add_scoped_pkg(&workspace, "foo", "@pnpm.e2e/foo", "999.0.0");

    stdout_of(pnpm_probing(&workspace).with_args([
        "change",
        "--bump",
        "minor",
        "--summary",
        "Initial release.",
        "@pnpm.e2e/foo",
    ]));
    let applied = stdout_of(pnpm_probing(&workspace).with_args(["version", "-r"]));
    assert!(applied.contains("@pnpm.e2e/foo: 999.0.0 → 999.0.0"), "unexpected: {applied}");
    assert_eq!(manifest_version(&workspace, "foo"), "999.0.0");

    drop(root);
}

/// A registry that cannot answer the probe (here an unroutable port, so the
/// check fails with a connection error rather than a 404) must fail
/// `pnpm change status` and `pnpm version -r` rather than guess a version.
/// Mirrors the TypeScript `a registry probe failure fails the command` test.
#[test]
fn first_release_probe_failure_fails_the_command() {
    let CommandTempCwd { workspace, root, .. } = CommandTempCwd::init();
    fs::write(workspace.join("pnpm-workspace.yaml"), "packages:\n  - packages/*\n")
        .expect("write yaml");
    fs::write(workspace.join(".npmrc"), "registry=http://127.0.0.1:1/\n").expect("write npmrc");
    fs::write(workspace.join("package.json"), "{\"name\": \"e2e-root\", \"private\": true}\n")
        .expect("write root package.json");
    add_scoped_pkg(&workspace, "foo", "@pnpm.e2e/foo", "1.2.0");

    // Recording an intent does not probe, so it succeeds despite the dead registry.
    stdout_of(pnpm_probing(&workspace).with_args([
        "change",
        "--bump",
        "minor",
        "--summary",
        "A feature.",
        "@pnpm.e2e/foo",
    ]));

    let status =
        pnpm_probing(&workspace).with_args(["change", "status"]).output().expect("run pnpm");
    assert!(!status.status.success(), "change status must fail when the probe errors");

    let release = pnpm_probing(&workspace).with_args(["version", "-r"]).output().expect("run pnpm");
    assert!(!release.status.success(), "version -r must fail when the probe errors");

    // No version was guessed: the manifest is untouched.
    assert_eq!(manifest_version(&workspace, "foo"), "1.2.0");

    drop(root);
}

fn write_workspace(workspace: &Path) {
    // These tests assert committed CHANGELOG.md files, which predate the
    // `registry`-storage default (where a release's section is parked under
    // `.changeset/changelogs/` and composed at publish time instead). Opt back
    // into `repository` storage so `pnpm version -r` writes the changelogs
    // inline, the way the assertions below expect.
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "packages:\n  - packages/*\nversioning:\n  changelog:\n    storage: repository\n",
    )
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
    // These tests exercise the change/version/lane engine, advancing manifests
    // without a real publish cycle. The first-release probe compares each
    // release's current version against the registry; assume every version is
    // already published so the engine bumps normally without a network round
    // trip. The probe's own behavior is covered against the mock registry by
    // the `first_release_probe_*` tests below.
    Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(workspace)
        .with_env("PACQUET_ASSUME_VERSIONS_PUBLISHED", "1")
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

/// A filtered `pnpm version -r` with nothing pending in scope must not
/// garbage-collect intents belonging to packages outside the filter.
#[test]
fn a_filtered_version_run_leaves_out_of_scope_intents_untouched() {
    let CommandTempCwd { workspace, root, .. } = CommandTempCwd::init();
    write_workspace(&workspace);
    add_pkg(&workspace, "lib", "1.0.0", "{}");
    add_pkg(&workspace, "cli", "2.0.0", "{}");

    stdout_of(pnpm(&workspace).with_args([
        "change",
        "--bump",
        "none",
        "--summary",
        "refactor, no release needed",
        "lib",
    ]));

    let output = stdout_of(pnpm(&workspace).with_args(["version", "-r", "--filter", "cli"]));
    assert!(output.contains("No pending changes"), "unexpected: {output}");
    let intents: Vec<_> = fs::read_dir(workspace.join(".changeset"))
        .expect("read .changeset")
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().ends_with(".md"))
        .collect();
    assert_eq!(intents.len(), 1, "the out-of-scope none-only intent must survive");

    drop(root);
}

/// `pnpm change status` is a read-only diagnostic: it must not fail with the
/// release-time workspace-protocol error, but `pnpm version -r` does enforce it.
#[test]
fn change_status_is_read_only_about_unmigrated_internal_deps() {
    let CommandTempCwd { workspace, root, .. } = CommandTempCwd::init();
    write_workspace(&workspace);
    add_pkg(&workspace, "lib", "1.0.0", "{}");
    add_pkg(&workspace, "cli", "2.0.0", r#"{"lib": "^1.0.0"}"#);

    let status = pnpm(&workspace).with_args(["change", "status"]).output().expect("run pnpm");
    assert!(
        status.status.success(),
        "change status must not fail: {}",
        String::from_utf8_lossy(&status.stderr),
    );

    stdout_of(pnpm(&workspace).with_args([
        "change",
        "--bump",
        "patch",
        "--summary",
        "A fix.",
        "lib",
    ]));
    let release = pnpm(&workspace).with_args(["version", "-r"]).output().expect("run pnpm");
    assert!(!release.status.success());
    assert!(
        String::from_utf8_lossy(&release.stderr).contains("workspace: protocol"),
        "unexpected: {}",
        String::from_utf8_lossy(&release.stderr),
    );

    drop(root);
}

/// Two workspace projects publishing the same npm name (pnpm's own TS and Rust
/// CLIs) must be referenced by directory; the ledger attributes the release to
/// the right one.
#[test]
fn a_name_shared_by_two_projects_must_be_referenced_by_directory() {
    let CommandTempCwd { workspace, root, .. } = CommandTempCwd::init();
    fs::write(workspace.join("pnpm-workspace.yaml"), "packages:\n  - ts/pnpm\n  - rust/pnpm\n")
        .expect("write pnpm-workspace.yaml");
    fs::write(workspace.join("package.json"), "{\"name\": \"e2e-root\", \"private\": true}\n")
        .expect("write root package.json");
    for (dir, version) in [("ts/pnpm", "11.0.0"), ("rust/pnpm", "12.0.0")] {
        let pkg_dir = workspace.join(dir);
        fs::create_dir_all(&pkg_dir).expect("create package dir");
        fs::write(
            pkg_dir.join("package.json"),
            format!("{{\"name\": \"pnpm\", \"version\": \"{version}\"}}\n"),
        )
        .expect("write package.json");
    }

    let output = pnpm(&workspace)
        .with_args(["change", "--bump", "patch", "--summary", "x", "pnpm"])
        .output()
        .expect("run pnpm");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("matches multiple workspace projects"), "unexpected: {stderr}");

    let recorded = stdout_of(pnpm(&workspace).with_args([
        "change",
        "--bump",
        "patch",
        "--summary",
        "Rust-line fix.",
        "./rust/pnpm",
    ]));
    assert!(recorded.contains("Recorded change intent"), "unexpected: {recorded}");

    let applied = stdout_of(pnpm(&workspace).with_args(["version", "-r"]));
    assert!(applied.contains("pnpm: 12.0.0 → 12.0.1"), "unexpected: {applied}");
    assert!(!applied.contains("11.0.0"), "the TS line must not release: {applied}");

    let rust_manifest: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(workspace.join("rust/pnpm/package.json")).expect("read"),
    )
    .expect("parse");
    assert_eq!(rust_manifest["version"].as_str(), Some("12.0.1"));

    let ledger = fs::read_to_string(workspace.join(".changeset/ledger.yaml")).expect("read ledger");
    assert!(ledger.contains("pnpm@12.0.1:"), "unexpected ledger: {ledger}");
    assert!(ledger.contains("dir: rust/pnpm"), "ledger must attribute by dir: {ledger}");

    drop(root);
}
