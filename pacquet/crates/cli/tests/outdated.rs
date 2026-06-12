use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pacquet_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use std::{ffi::OsStr, fs, path::Path, process::Command};
use tempfile::TempDir;

const DEP: &str = "@pnpm.e2e/dep-of-pkg-with-1-dep";
const FOO: &str = "@pnpm.e2e/foo";
const DEPRECATED: &str = "@pnpm.e2e/deprecated";

fn setup() -> (TempDir, std::path::PathBuf, AddMockedRegistry) {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    (root, workspace, npmrc_info)
}

fn pacquet(workspace: &Path, args: impl IntoIterator<Item = impl AsRef<OsStr>>) -> Command {
    Command::cargo_bin("pacquet")
        .expect("find the pacquet binary")
        .with_current_dir(workspace)
        .with_args(args)
}

fn write_manifest(workspace: &Path, dependencies: &str) {
    let manifest = format!(
        r#"{{ "name": "test-outdated", "version": "1.0.0", "dependencies": {dependencies} }}"#,
    );
    fs::write(workspace.join("package.json"), manifest).expect("write package.json");
}

/// `outdated` reports a dependency whose `latest` tag is newer than the
/// installed (in-range) version, and exits non-zero.
#[test]
fn outdated_reports_newer_version() {
    let (root, workspace, anchor) = setup();

    // `^100.0.0` installs the highest in-range version (100.1.0); the
    // `latest` tag is 101.0.0, so the dependency is outdated.
    write_manifest(&workspace, &format!(r#"{{ "{DEP}": "^100.0.0" }}"#));
    pacquet(&workspace, ["install"]).assert().success();

    let output = pacquet(&workspace, ["outdated"]).output().expect("run pacquet outdated");
    assert_eq!(output.status.code(), Some(1), "outdated deps present should exit 1");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains(DEP), "report should mention the package: {stdout}");
    assert!(stdout.contains("100.1.0"), "report should show the current version: {stdout}");
    assert!(stdout.contains("101.0.0"), "report should show the latest version: {stdout}");

    drop((root, anchor));
}

/// `--compatible` compares against the highest in-range version, so a
/// dependency already at the top of its range is not reported even when a
/// newer major exists.
#[test]
fn outdated_compatible_ignores_out_of_range_releases() {
    let (root, workspace, anchor) = setup();

    write_manifest(&workspace, &format!(r#"{{ "{DEP}": "^100.0.0" }}"#));
    pacquet(&workspace, ["install"]).assert().success();

    // Default run is outdated (101.0.0 latest)...
    assert_eq!(
        pacquet(&workspace, ["outdated"]).output().unwrap().status.code(),
        Some(1),
        "default outdated should flag the out-of-range 101.0.0",
    );

    // ...but --compatible only considers in-range versions; 100.1.0 is
    // already the highest in `^100.0.0`, so nothing is outdated.
    let output =
        pacquet(&workspace, ["outdated", "--compatible"]).output().expect("run pacquet outdated");
    assert_eq!(output.status.code(), Some(0), "compatible run should be up to date");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.contains(DEP), "compatible run should report nothing: {stdout}");

    drop((root, anchor));
}

/// `--format json` emits a parseable object keyed by package name.
#[test]
fn outdated_json_format() {
    let (root, workspace, anchor) = setup();

    write_manifest(&workspace, &format!(r#"{{ "{DEP}": "^100.0.0" }}"#));
    pacquet(&workspace, ["install"]).assert().success();

    let output = pacquet(&workspace, ["outdated", "--format", "json"])
        .output()
        .expect("run pacquet outdated");
    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let value: serde_json::Value =
        serde_json::from_str(stdout.trim()).expect("outdated --format json should emit valid JSON");
    let entry = &value[DEP];
    assert_eq!(entry["current"], "100.1.0");
    assert_eq!(entry["latest"], "101.0.0");
    assert_eq!(entry["dependencyType"], "dependencies");

    drop((root, anchor));
}

/// A dependency pinned to its `latest` tag is not outdated; the report is
/// empty and the exit code is zero.
#[test]
fn outdated_up_to_date_exits_zero() {
    let (root, workspace, anchor) = setup();

    write_manifest(&workspace, &format!(r#"{{ "{DEP}": "101.0.0" }}"#));
    pacquet(&workspace, ["install"]).assert().success();

    let output = pacquet(&workspace, ["outdated"]).output().expect("run pacquet outdated");
    assert_eq!(output.status.code(), Some(0), "up-to-date deps should exit 0");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.contains(DEP), "no outdated dep should be reported: {stdout}");

    drop((root, anchor));
}

/// A package selector restricts the report to matching dependencies.
#[test]
fn outdated_pattern_filters_dependencies() {
    let (root, workspace, anchor) = setup();

    write_manifest(&workspace, &format!(r#"{{ "{DEP}": "^100.0.0", "{FOO}": "^1.0.0" }}"#));
    pacquet(&workspace, ["install"]).assert().success();

    let output = pacquet(&workspace, ["outdated", FOO]).output().expect("run pacquet outdated");
    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains(FOO), "selected package should be reported: {stdout}");
    assert!(!stdout.contains(DEP), "unselected package should be excluded: {stdout}");

    drop((root, anchor));
}

/// A package at its newest version but marked deprecated is still
/// reported as outdated.
#[test]
fn outdated_reports_deprecated_package() {
    let (root, workspace, anchor) = setup();

    write_manifest(&workspace, &format!(r#"{{ "{DEPRECATED}": "1.0.0" }}"#));
    pacquet(&workspace, ["install"]).assert().success();

    let output = pacquet(&workspace, ["outdated"]).output().expect("run pacquet outdated");
    assert_eq!(output.status.code(), Some(1), "deprecated dep should be flagged");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains(DEPRECATED), "deprecated package should be reported: {stdout}");
    assert!(stdout.contains("Deprecated"), "should mark the version deprecated: {stdout}");

    drop((root, anchor));
}

/// Without a lockfile, `outdated` fails with
/// `ERR_PNPM_OUTDATED_NO_LOCKFILE` rather than silently reporting nothing.
#[test]
fn outdated_without_lockfile_errors() {
    let (root, workspace, anchor) = setup();

    write_manifest(&workspace, &format!(r#"{{ "{DEP}": "^100.0.0" }}"#));

    let output = pacquet(&workspace, ["outdated"]).output().expect("run pacquet outdated");
    assert!(!output.status.success(), "outdated without a lockfile should fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("No lockfile in directory"), "stderr should explain why: {stderr}");

    drop((root, anchor));
}

/// `--recursive` is rejected until workspace-wide inspection is ported.
#[test]
fn outdated_recursive_is_rejected() {
    let (root, workspace, anchor) = setup();

    write_manifest(&workspace, &format!(r#"{{ "{DEP}": "^100.0.0" }}"#));
    pacquet(&workspace, ["install"]).assert().success();

    let output =
        pacquet(&workspace, ["outdated", "--recursive"]).output().expect("run pacquet outdated");
    assert!(!output.status.success(), "recursive outdated should be rejected");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("not supported yet"), "stderr should say it is unsupported: {stderr}");

    drop((root, anchor));
}

/// A dependency-free manifest is reported as up to date (exit 0) even
/// when there is no lockfile — the no-lockfile error only fires for a
/// manifest that actually declares dependencies. Ports pnpm's "should
/// return empty when there is no lockfile and no dependencies".
#[test]
fn outdated_no_dependencies_no_lockfile_is_empty() {
    let (root, workspace, anchor) = setup();

    fs::write(workspace.join("package.json"), r#"{ "name": "test-outdated", "version": "1.0.0" }"#)
        .expect("write package.json");

    let output = pacquet(&workspace, ["outdated"]).output().expect("run pacquet outdated");
    assert_eq!(output.status.code(), Some(0), "no dependencies should exit 0");
    assert!(String::from_utf8_lossy(&output.stdout).trim().is_empty(), "report should be empty");

    drop((root, anchor));
}

/// `--format json` emits `{}` (exit 0) when nothing is outdated. Ports
/// pnpm's "format json when there are no outdated dependencies".
#[test]
fn outdated_json_empty_when_up_to_date() {
    let (root, workspace, anchor) = setup();

    write_manifest(&workspace, &format!(r#"{{ "{DEP}": "101.0.0" }}"#));
    pacquet(&workspace, ["install"]).assert().success();

    let output = pacquet(&workspace, ["outdated", "--format", "json"])
        .output()
        .expect("run pacquet outdated");
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "{}");

    drop((root, anchor));
}

/// `--no-table` (list format) prints names and `=>` arrows instead of a
/// box-drawing table. Ports pnpm's "no table".
#[test]
fn outdated_list_format() {
    let (root, workspace, anchor) = setup();

    write_manifest(&workspace, &format!(r#"{{ "{DEP}": "^100.0.0" }}"#));
    pacquet(&workspace, ["install"]).assert().success();

    let output =
        pacquet(&workspace, ["outdated", "--no-table"]).output().expect("run pacquet outdated");
    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains(DEP), "list should mention the package: {stdout}");
    assert!(stdout.contains("=>"), "list uses the `=>` arrow: {stdout}");
    assert!(!stdout.contains('┌'), "list format must not draw a table: {stdout}");

    drop((root, anchor));
}

/// `--long` adds the deprecation reason to the report. Ports pnpm's
/// "--long with only deprecated packages".
#[test]
fn outdated_long_shows_deprecation_details() {
    let (root, workspace, anchor) = setup();

    write_manifest(&workspace, &format!(r#"{{ "{DEPRECATED}": "1.0.0" }}"#));
    pacquet(&workspace, ["install"]).assert().success();

    let output =
        pacquet(&workspace, ["outdated", "--long"]).output().expect("run pacquet outdated");
    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("This package is deprecated"),
        "--long should print the deprecation reason: {stdout}",
    );

    drop((root, anchor));
}

/// An npm-aliased dependency is reported under its real registry name,
/// not the alias. Ports pnpm's "`outdated()` aliased dependency".
#[test]
fn outdated_npm_alias_reports_real_name() {
    let (root, workspace, anchor) = setup();

    write_manifest(&workspace, &format!(r#"{{ "positive": "npm:{FOO}@^1.0.0" }}"#));
    pacquet(&workspace, ["install"]).assert().success();

    let output = pacquet(&workspace, ["outdated"]).output().expect("run pacquet outdated");
    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains(FOO), "report should use the real package name: {stdout}");

    drop((root, anchor));
}

/// `--prod` and `--dev` restrict the report to the matching dependency
/// group. Ports pnpm's "showing only prod or dev dependencies".
#[test]
fn outdated_prod_dev_filtering() {
    let (root, workspace, anchor) = setup();

    fs::write(
        workspace.join("package.json"),
        format!(
            r#"{{ "name": "test-outdated", "version": "1.0.0", "dependencies": {{ "{DEP}": "^100.0.0" }}, "devDependencies": {{ "{FOO}": "^1.0.0" }} }}"#,
        ),
    )
    .expect("write package.json");
    pacquet(&workspace, ["install"]).assert().success();

    let prod = pacquet(&workspace, ["outdated", "--prod"]).output().expect("run pacquet outdated");
    let prod_out = String::from_utf8_lossy(&prod.stdout);
    assert!(prod_out.contains(DEP), "--prod includes the prod dep: {prod_out}");
    assert!(!prod_out.contains(FOO), "--prod excludes the dev dep: {prod_out}");

    let dev = pacquet(&workspace, ["outdated", "--dev"]).output().expect("run pacquet outdated");
    let dev_out = String::from_utf8_lossy(&dev.stdout);
    assert!(dev_out.contains(FOO), "--dev includes the dev dep: {dev_out}");
    assert!(!dev_out.contains(DEP), "--dev excludes the prod dep: {dev_out}");

    drop((root, anchor));
}
