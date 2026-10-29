use crate::_utils::{
    append_workspace_yaml_key, bravo_dep_mature_up_to_1_0_1_minimum_release_age,
    set_ignore_dependencies, set_minimum_release_age,
};
use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::{
    bin::{AddMockedRegistry, CommandTempCwd},
    command_env::CommandTestExt,
};
use std::{
    ffi::OsStr,
    fs, io,
    path::Path,
    process::{Command, Stdio},
};
use tempfile::TempDir;

const DEP: &str = "@pnpm.e2e/dep-of-pkg-with-1-dep";
const FOO: &str = "@pnpm.e2e/foo";
const DEPRECATED: &str = "@pnpm.e2e/deprecated";
const BRAVO_DEP: &str = "@pnpm.e2e/bravo-dep";

fn setup() -> (TempDir, std::path::PathBuf, AddMockedRegistry) {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    (root, workspace, npmrc_info)
}

fn pacquet(workspace: &Path, args: impl IntoIterator<Item = impl AsRef<OsStr>>) -> Command {
    Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(workspace)
        .without_ambient_pnpm_config()
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

/// The exit status is the verdict a CI pipeline acts on, so a closed
/// stdout drops the report and keeps the status (pnpm/tasks#132).
#[test]
fn outdated_keeps_its_exit_code_when_stdout_is_closed() {
    let (root, workspace, anchor) = setup();
    write_manifest(&workspace, &format!(r#"{{ "{DEP}": "^100.0.0" }}"#));
    pacquet(&workspace, ["install"]).assert().success();
    let (stdout_reader, stdout_writer) = io::pipe().expect("create stdout pipe");
    drop(stdout_reader);

    let output = pacquet(&workspace, ["outdated"])
        .with_stdout(Stdio::from(stdout_writer))
        .output()
        .expect("run pacquet outdated");
    assert_eq!(output.status.code(), Some(1), "{output:?}");

    drop((root, anchor));
}

#[test]
fn outdated_from_workspace_member_reads_member_importer() {
    let (root, workspace, anchor) = setup();
    fs::write(workspace.join("pnpm-workspace.yaml"), "packages:\n  - packages/*\n")
        .expect("write workspace manifest");
    write_manifest(&workspace, "{}");
    let member = workspace.join("packages/app");
    fs::create_dir_all(&member).expect("create workspace member");
    fs::write(
        member.join("package.json"),
        format!(
            r#"{{ "name": "app", "version": "1.0.0", "dependencies": {{ "{DEP}": "^100.0.0" }} }}"#,
        ),
    )
    .expect("write member package.json");
    pacquet(&workspace, ["install"]).assert().success();

    let output = pacquet(&member, ["outdated"]).output().expect("run member outdated");

    assert_eq!(output.status.code(), Some(1), "member dependency should be outdated");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains(DEP), "report should use the member importer: {stdout}");
    assert!(
        stdout.contains("100.1.0"),
        "report should show the member's current version: {stdout}",
    );

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
        pacquet(&workspace, ["outdated"])
            .output()
            .unwrap()
            .status
            .code(),
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

/// Covers <https://github.com/pnpm/pnpm/issues/14004>: `outdated` must offer
/// mature releases without offering newer versions blocked by `minimumReleaseAge`.
#[test]
fn outdated_respects_minimum_release_age() {
    let (root, workspace, anchor) = setup();

    write_manifest(&workspace, &format!(r#"{{ "{BRAVO_DEP}": "1.0.0" }}"#));
    set_minimum_release_age(&workspace, bravo_dep_mature_up_to_1_0_1_minimum_release_age());
    pacquet(&workspace, ["install"]).assert().success();

    let output = pacquet(&workspace, ["outdated"]).output().expect("run pacquet outdated");
    assert_eq!(output.status.code(), Some(1), "the mature release should be offered");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains(BRAVO_DEP), "report should include the package: {stdout}");
    assert!(stdout.contains("1.0.1"), "report should offer the mature release: {stdout}");
    assert!(!stdout.contains("1.1.0"), "report should omit the immature release: {stdout}");

    write_manifest(&workspace, &format!(r#"{{ "{BRAVO_DEP}": "1.0.1" }}"#));
    pacquet(&workspace, ["install"]).assert().success();
    let output = pacquet(&workspace, ["outdated"]).output().expect("run pacquet outdated");
    assert_eq!(output.status.code(), Some(0), "immature releases should not be offered");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.contains(BRAVO_DEP), "report should omit immature releases: {stdout}");

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

#[test]
fn recursive_outdated_reports_the_shared_lockfile_directory() {
    let (root, workspace, anchor) = setup();
    fs::write(workspace.join("pnpm-workspace.yaml"), "packages:\n  - packages/*\n")
        .expect("write workspace manifest");
    write_manifest(&workspace, "{}");
    let project = workspace.join("packages/app");
    fs::create_dir_all(&project).expect("create workspace project");
    fs::write(
        project.join("package.json"),
        format!(
            r#"{{ "name": "app", "version": "1.0.0", "dependencies": {{ "{DEP}": "^100.0.0" }} }}"#,
        ),
    )
    .expect("write project manifest");

    let output = pacquet(&workspace, ["outdated", "--recursive"])
        .output()
        .expect("run recursive outdated without a shared lockfile");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(r#"workspace""#),
        "error should point to the shared lockfile directory: {stderr}",
    );
    assert!(
        !stderr.contains(r#"app""#),
        "error should not point to a project-specific lockfile: {stderr}",
    );

    drop((root, anchor));
}

#[test]
fn outdated_recursive_aggregates_workspace_dependents() {
    let (root, workspace, anchor) = setup();
    fs::write(workspace.join("pnpm-workspace.yaml"), "packages:\n  - packages/*\n")
        .expect("write workspace manifest");
    write_manifest(&workspace, "{}");
    for name in ["app-a", "app-b"] {
        let project = workspace.join("packages").join(name);
        fs::create_dir_all(&project).expect("create workspace project");
        fs::write(
            project.join("package.json"),
            format!(
                r#"{{ "name": "{name}", "version": "1.0.0", "dependencies": {{ "{DEP}": "^100.0.0" }} }}"#,
            ),
        )
        .expect("write project manifest");
    }
    pacquet(&workspace, ["install"]).assert().success();

    let output = pacquet(&workspace, ["outdated", "--recursive", "--format", "json"])
        .output()
        .expect("run recursive outdated");

    assert_eq!(output.status.code(), Some(1));
    let value: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("recursive outdated should emit valid JSON");
    let dependents = value[DEP]["dependentPackages"].as_array().expect("dependent package list");
    assert_eq!(dependents.len(), 2);
    assert_eq!(dependents[0]["name"], "app-a");
    assert_eq!(dependents[1]["name"], "app-b");
    assert!(
        dependents[0]["location"]
            .as_str()
            .is_some_and(|path| Path::new(path).ends_with(Path::new("packages").join("app-a"))),
        "unexpected app-a location: {}",
        dependents[0]["location"],
    );
    assert!(
        dependents[1]["location"]
            .as_str()
            .is_some_and(|path| Path::new(path).ends_with(Path::new("packages").join("app-b"))),
        "unexpected app-b location: {}",
        dependents[1]["location"],
    );

    let filtered =
        pacquet(&workspace, ["--filter", "app-a", "outdated", "--recursive", "--format", "json"])
            .output()
            .expect("run filtered recursive outdated");
    assert_eq!(filtered.status.code(), Some(1));
    let value: serde_json::Value = serde_json::from_slice(&filtered.stdout)
        .expect("filtered recursive outdated should emit valid JSON");
    assert_eq!(value[DEP]["dependentPackages"].as_array().map(Vec::len), Some(1));
    assert_eq!(value[DEP]["dependentPackages"][0]["name"], "app-a");

    drop((root, anchor));
}

#[test]
fn outdated_recursive_json_preserves_distinct_entries_for_same_package() {
    let (root, workspace, anchor) = setup();
    fs::write(workspace.join("pnpm-workspace.yaml"), "packages:\n  - packages/*\n")
        .expect("write workspace manifest");
    write_manifest(&workspace, "{}");

    let app_a = workspace.join("packages/app-a");
    fs::create_dir_all(&app_a).expect("create app-a");
    fs::write(
        app_a.join("package.json"),
        format!(
            r#"{{ "name": "app-a", "version": "1.0.0", "dependencies": {{ "{DEP}": "^100.0.0" }} }}"#,
        ),
    )
    .expect("write app-a package.json");

    let app_b = workspace.join("packages/app-b");
    fs::create_dir_all(&app_b).expect("create app-b");
    fs::write(
        app_b.join("package.json"),
        format!(
            r#"{{ "name": "app-b", "version": "1.0.0", "devDependencies": {{ "{DEP}": "^100.0.0" }} }}"#,
        ),
    )
    .expect("write app-b package.json");

    pacquet(&workspace, ["install"]).assert().success();

    let output = pacquet(&workspace, ["outdated", "--recursive", "--format", "json"])
        .output()
        .expect("run recursive outdated");

    assert_eq!(output.status.code(), Some(1));
    let value: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("recursive outdated should emit valid JSON");

    let prod_key = format!("{DEP}@100.1.0");
    let dev_key = format!("{DEP}@100.1.0 (dev)");

    assert!(value.get(&prod_key).is_some(), "expected prod key {prod_key} in {value}");
    assert!(value.get(&dev_key).is_some(), "expected dev key {dev_key} in {value}");

    assert_eq!(value[&prod_key]["dependencyType"], "dependencies");
    assert_eq!(value[&dev_key]["dependencyType"], "devDependencies");

    let prod_dependents =
        value[&prod_key]["dependentPackages"].as_array().expect("prod dependents");
    assert_eq!(prod_dependents.len(), 1);
    assert_eq!(prod_dependents[0]["name"], "app-a");

    let dev_dependents = value[&dev_key]["dependentPackages"].as_array().expect("dev dependents");
    assert_eq!(dev_dependents.len(), 1);
    assert_eq!(dev_dependents[0]["name"], "app-b");

    drop((root, anchor));
}

#[test]
fn outdated_recursive_reads_dedicated_project_lockfiles() {
    let (root, workspace, anchor) = setup();
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "packages:\n  - packages/*\nsharedWorkspaceLockfile: false\n",
    )
    .expect("write workspace manifest");
    write_manifest(&workspace, "{}");
    let project = workspace.join("packages/app");
    fs::create_dir_all(&project).expect("create workspace project");
    fs::write(
        project.join("package.json"),
        format!(
            r#"{{ "name": "app", "version": "1.0.0", "dependencies": {{ "{DEP}": "^100.0.0" }} }}"#,
        ),
    )
    .expect("write project manifest");
    pacquet(&workspace, ["install"]).assert().success();

    let output = pacquet(&workspace, ["outdated", "--recursive"])
        .output()
        .expect("run recursive outdated with dedicated lockfiles");

    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains(DEP), "report should mention the package: {stdout}");
    assert!(stdout.contains("app"), "report should name the dependent: {stdout}");

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

/// Ports pnpm's "--long with only deprecated packages".
#[test]
fn outdated_long_shows_deprecation_and_homepage_details() {
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
    assert!(stdout.contains("https://foo.bar/qar"), "--long should print the homepage: {stdout}");

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

/// An override that redirects a dependency to an npm alias is checked
/// against the alias target's versions, not the original package's.
#[test]
fn outdated_npm_alias_override_checks_the_alias_target() {
    let (root, workspace, anchor) = setup();

    append_workspace_yaml_key(
        &workspace,
        "overrides",
        format!("{{ '{DEP}': 'npm:{FOO}@100.1.0' }}"),
    );
    write_manifest(&workspace, &format!(r#"{{ "{DEP}": "^100.0.0" }}"#));
    pacquet(&workspace, ["install"]).assert().success();

    let output = pacquet(&workspace, ["outdated"]).output().expect("run pacquet outdated");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(output.status.code(), Some(0), "the alias target is at its latest: {stdout}");

    drop((root, anchor));
}

/// A project's own overrides, set through `packageConfigs`, apply to it
/// when it has a lockfile of its own.
#[test]
fn outdated_recursive_applies_project_overrides() {
    let (root, workspace, anchor) = setup();
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        format!(
            "packages:\n  - packages/*\nsharedWorkspaceLockfile: false\npackageConfigs:\n  app:\n    overrides:\n      '{DEP}': 'npm:{FOO}@100.1.0'\n",
        ),
    )
    .expect("write workspace manifest");
    write_manifest(&workspace, "{}");
    let project = workspace.join("packages/app");
    fs::create_dir_all(&project).expect("create workspace project");
    fs::write(
        project.join("package.json"),
        format!(
            r#"{{ "name": "app", "version": "1.0.0", "dependencies": {{ "{DEP}": "^100.0.0" }} }}"#,
        ),
    )
    .expect("write project manifest");
    pacquet(&workspace, ["install"]).assert().success();

    let output = pacquet(&workspace, ["outdated", "--recursive"])
        .output()
        .expect("run recursive outdated with dedicated lockfiles");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(output.status.code(), Some(0), "the alias target is at its latest: {stdout}");

    drop((root, anchor));
}

/// A `catalog:` dependency is checked against the specifier the catalog
/// holds, so an npm-aliased catalog entry is reported under the real
/// registry name instead of failing on the alias key.
#[test]
fn outdated_catalog_npm_alias_reports_real_name() {
    let (root, workspace, anchor) = setup();

    append_workspace_yaml_key(&workspace, "catalog", format!("{{ positive: 'npm:{FOO}@^1.0.0' }}"));
    write_manifest(&workspace, r#"{ "positive": "catalog:" }"#);
    pacquet(&workspace, ["install"]).assert().success();

    let output = pacquet(&workspace, ["outdated"]).output().expect("run pacquet outdated");
    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains(FOO), "report should use the catalog entry's package name: {stdout}");

    drop((root, anchor));
}

/// `--compatible` compares against the range the catalog entry holds.
#[test]
fn outdated_compatible_uses_the_catalog_range() {
    let (root, workspace, anchor) = setup();

    append_workspace_yaml_key(&workspace, "catalog", format!("{{ '{DEP}': '100.0.0' }}"));
    write_manifest(&workspace, &format!(r#"{{ "{DEP}": "catalog:" }}"#));
    pacquet(&workspace, ["install"]).assert().success();

    // Widening the catalog range without reinstalling leaves the lockfile
    // pinned at 100.0.0 while 100.1.0 is now in range.
    let workspace_yaml = workspace.join("pnpm-workspace.yaml");
    let yaml = fs::read_to_string(&workspace_yaml).expect("read pnpm-workspace.yaml");
    fs::write(&workspace_yaml, yaml.replace("'100.0.0'", "'^100.0.0'"))
        .expect("widen the catalog range");

    let output =
        pacquet(&workspace, ["outdated", "--compatible"]).output().expect("run pacquet outdated");
    assert_eq!(output.status.code(), Some(1), "the in-range 100.1.0 should be reported");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("100.1.0"), "report should show the in-range version: {stdout}");

    drop((root, anchor));
}

/// A dependency left on `catalog:` after its catalog entry is gone
/// fails with pnpm's catalog error, not with whatever the registry
/// answers for the bare dependency key.
#[test]
fn outdated_catalog_entry_missing_is_a_catalog_error() {
    let (root, workspace, anchor) = setup();

    append_workspace_yaml_key(&workspace, "catalog", format!("{{ '{DEP}': '^100.0.0' }}"));
    write_manifest(&workspace, &format!(r#"{{ "{DEP}": "catalog:" }}"#));
    pacquet(&workspace, ["install"]).assert().success();

    let workspace_yaml = workspace.join("pnpm-workspace.yaml");
    let yaml = fs::read_to_string(&workspace_yaml).expect("read pnpm-workspace.yaml");
    let without_catalog = yaml
        .lines()
        .filter(|line| !line.starts_with("catalog:"))
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(&workspace_yaml, without_catalog).expect("drop the catalog entry");

    let output = pacquet(&workspace, ["outdated"]).output().expect("run pacquet outdated");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(1), "the run should fail: {stderr}");
    assert!(
        stderr.contains("ERR_PNPM_CATALOG_ENTRY_NOT_FOUND_FOR_SPEC"),
        "missing catalog entry should be reported as such: {stderr}",
    );

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

/// Ports `ignore packages in package.json > pnpm.updateConfig.ignoreDependencies
/// in outdated command`.
#[test]
fn outdated_leaves_out_ignored_dependencies() {
    let (root, workspace, anchor) = setup();

    write_manifest(&workspace, &format!(r#"{{ "{DEP}": "^100.0.0", "{FOO}": "^1.0.0" }}"#));
    pacquet(&workspace, ["install"]).assert().success();
    set_ignore_dependencies(&workspace, &[FOO]);

    let output = pacquet(&workspace, ["outdated"]).output().expect("run pacquet outdated");

    assert_eq!(output.status.code(), Some(1), "the unignored dependency is still outdated");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains(DEP), "report should mention the unignored package: {stdout}");
    assert!(!stdout.contains(FOO), "report should leave out the ignored package: {stdout}");

    drop((root, anchor));
}

#[test]
fn outdated_fails_when_package_not_in_dependencies() {
    let (root, workspace, anchor) = setup();

    fs::write(
        workspace.join("package.json"),
        format!(
            r#"{{ "name": "test-outdated", "version": "1.0.0", "dependencies": {{ "{DEP}": "^100.0.0" }}, "devDependencies": {{ "{FOO}": "^1.0.0" }} }}"#,
        ),
    )
    .expect("write package.json");
    pacquet(&workspace, ["install"]).assert().success();

    for args in [
        vec!["outdated", "not-a-dep"],
        vec!["outdated", DEP, "not-a-dep"],
        vec!["outdated", "--prod", FOO],
        vec!["outdated", "--dev", DEP],
    ] {
        let output = pacquet(&workspace, &args).output().expect("run pacquet outdated");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(!output.status.success(), "{args:?} must fail: {stderr}");
        assert!(
            stderr.contains("ERR_PNPM_NO_PACKAGE_IN_DEPENDENCIES"),
            "{args:?} must report ERR_PNPM_NO_PACKAGE_IN_DEPENDENCIES: {stderr}",
        );
        assert!(
            stderr.contains("None of the specified packages were found in the dependencies."),
            "{args:?} must report single-project message: {stderr}",
        );
    }

    let compound = pacquet(&workspace, ["outdated", "!not-a-dep", DEP])
        .output()
        .expect("run pacquet outdated with compound pattern");
    assert_eq!(compound.status.code(), Some(1));

    let excluded_only = pacquet(&workspace, ["outdated", &format!("!{DEP}"), &format!("!{FOO}")])
        .output()
        .expect("run pacquet outdated with only excluded pattern");
    assert_eq!(excluded_only.status.code(), Some(0));

    let cancelled = pacquet(&workspace, ["outdated", DEP, &format!("!{DEP}")])
        .output()
        .expect("run pacquet outdated with cancelled pattern");
    assert_eq!(cancelled.status.code(), Some(0));

    fs::remove_file(workspace.join("pnpm-lock.yaml")).expect("remove lockfile");
    let no_lockfile = pacquet(&workspace, ["outdated", "not-a-dep"])
        .output()
        .expect("run pacquet outdated without lockfile");
    let stderr = String::from_utf8_lossy(&no_lockfile.stderr);
    assert!(!no_lockfile.status.success());
    assert!(
        stderr.contains("ERR_PNPM_NO_PACKAGE_IN_DEPENDENCIES"),
        "must fail with ERR_PNPM_NO_PACKAGE_IN_DEPENDENCIES before checking lockfile: {stderr}",
    );

    drop((root, anchor));
}

#[test]
fn outdated_recursive_validates_workspace_dependencies() {
    let (root, workspace, anchor) = setup();

    fs::write(workspace.join("pnpm-workspace.yaml"), "packages:\n  - packages/*\n")
        .expect("write pnpm-workspace.yaml");
    write_manifest(&workspace, "{}");
    let pkg_a = workspace.join("packages/app-a");
    let pkg_b = workspace.join("packages/app-b");
    fs::create_dir_all(&pkg_a).expect("create app-a");
    fs::create_dir_all(&pkg_b).expect("create app-b");
    fs::write(
        pkg_a.join("package.json"),
        format!(
            r#"{{ "name": "app-a", "version": "1.0.0", "dependencies": {{ "{DEP}": "^100.0.0" }} }}"#,
        ),
    )
    .expect("write app-a package.json");
    fs::write(
        pkg_b.join("package.json"),
        format!(
            r#"{{ "name": "app-b", "version": "1.0.0", "devDependencies": {{ "{FOO}": "^1.0.0" }} }}"#,
        ),
    )
    .expect("write app-b package.json");

    pacquet(&workspace, ["install"]).assert().success();

    let spread = pacquet(&workspace, ["outdated", "-r", DEP, FOO])
        .output()
        .expect("run recursive outdated across projects");
    assert_eq!(spread.status.code(), Some(1));

    for args in [
        vec!["outdated", "-r", "not-a-dep"],
        vec!["outdated", "-r", DEP, "not-a-dep"],
        vec!["outdated", "-r", "--prod", FOO],
        vec!["--filter", "app-a", "outdated", "-r", FOO],
    ] {
        let output = pacquet(&workspace, &args).output().expect("run recursive outdated");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(!output.status.success(), "{args:?} must fail: {stderr}");
        assert!(
            stderr.contains("ERR_PNPM_NO_PACKAGE_IN_DEPENDENCIES"),
            "{args:?} must report ERR_PNPM_NO_PACKAGE_IN_DEPENDENCIES: {stderr}",
        );
        assert!(
            stderr.contains(
                "None of the specified packages were found in the dependencies of any of"
            ),
            "{args:?} must report workspace message: {stderr}",
        );
    }

    let empty_filter =
        pacquet(&workspace, ["--filter", "absent-project", "outdated", "-r", "not-a-dep"])
            .output()
            .expect("run recursive outdated with empty filter");
    assert!(empty_filter.status.success(), "empty filter selection should exit 0");

    let compound = pacquet(&workspace, ["outdated", "-r", "!not-a-dep", DEP])
        .output()
        .expect("run recursive outdated with compound pattern");
    assert_eq!(compound.status.code(), Some(1));

    let excluded_only =
        pacquet(&workspace, ["outdated", "-r", &format!("!{DEP}"), &format!("!{FOO}")])
            .output()
            .expect("run recursive outdated with only excluded patterns");
    assert_eq!(excluded_only.status.code(), Some(0));

    let cancelled = pacquet(&workspace, ["outdated", "-r", DEP, &format!("!{DEP}")])
        .output()
        .expect("run recursive outdated with cancelled pattern");
    assert_eq!(cancelled.status.code(), Some(0));

    drop((root, anchor));
}

#[test]
fn outdated_reports_package_manager_and_runtime_from_dev_engines() {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    fs::create_dir(&workspace).unwrap();
    let mut server = mockito::Server::new();
    let server_url = server.url();
    let _pnpm_mock = server
        .mock("GET", "/npm/pnpm")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            serde_json::json!({
                "name": "pnpm",
                "dist-tags": { "latest": "11.0.0" },
                "versions": {
                    "10.0.0": {
                        "name": "pnpm",
                        "version": "10.0.0",
                        "dist": { "tarball": format!("{server_url}/npm/pnpm/-/pnpm-10.0.0.tgz") },
                    },
                    "10.5.0": {
                        "name": "pnpm",
                        "version": "10.5.0",
                        "dist": { "tarball": format!("{server_url}/npm/pnpm/-/pnpm-10.5.0.tgz") },
                    },
                    "11.0.0": {
                        "name": "pnpm",
                        "version": "11.0.0",
                        "dist": { "tarball": format!("{server_url}/npm/pnpm/-/pnpm-11.0.0.tgz") },
                    },
                },
            })
            .to_string(),
        )
        .expect_at_least(1)
        .create();
    let _node_mock = server
        .mock("GET", "/node/index.json")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"[{"version":"v24.0.0","lts":false},{"version":"v22.5.0","lts":false},{"version":"v22.0.0","lts":false}]"#,
        )
        .expect_at_least(1)
        .create();

    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        format!(
            "storeDir: ../store\ncacheDir: ../cache\nnodeDownloadMirrors:\n  release: '{}/node/'\n",
            server.url(),
        ),
    )
    .unwrap();
    fs::write(workspace.join(".npmrc"), format!("registry={}/npm/\n", server.url())).unwrap();
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "name": "test-outdated-engines",
            "version": "1.0.0",
            "devEngines": {
                "packageManager": {
                    "name": "pnpm",
                    "version": "^10.0.0",
                    "onFail": "warn",
                },
                "runtime": {
                    "name": "node",
                    "version": "^22.0.0",
                    "onFail": "warn",
                },
            },
        })
        .to_string(),
    )
    .unwrap();

    let output = pacquet(&workspace, ["outdated", "--format", "json"])
        .output()
        .expect("run outdated --format json");
    assert_eq!(
        output.status.code(),
        Some(1),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr),
    );
    let report: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("parse outdated JSON");
    assert_eq!(report["pnpm"]["current"], "10.0.0");
    assert_eq!(report["pnpm"]["latest"], "11.0.0");
    assert_eq!(report["pnpm"]["dependencyType"], "devDependencies");
    assert_eq!(report["node"]["current"], "22.0.0");
    assert_eq!(report["node"]["latest"], "24.0.0");
    assert_eq!(report["node"]["dependencyType"], "devDependencies");

    let compat_output = pacquet(&workspace, ["outdated", "--compatible", "--format", "json"])
        .output()
        .expect("run outdated --compatible --format json");
    assert_eq!(compat_output.status.code(), Some(1));
    let compat_report: serde_json::Value =
        serde_json::from_slice(&compat_output.stdout).expect("parse compatible outdated JSON");
    assert_eq!(compat_report["pnpm"]["current"], "10.0.0");
    assert_eq!(compat_report["pnpm"]["latest"], "10.5.0");
    assert_eq!(compat_report["node"]["current"], "22.0.0");
    assert_eq!(compat_report["node"]["latest"], "22.5.0");

    // Selector filtering works even when `onFail` would otherwise block or switch.
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "name": "test-outdated-engines",
            "version": "1.0.0",
            "packageManager": "pnpm@10.0.0+sha256.deadbeef",
        })
        .to_string(),
    )
    .unwrap();
    let selector_output = pacquet(&workspace, ["outdated", "pnpm", "--format", "json"])
        .output()
        .expect("run outdated pnpm --format json");
    assert_eq!(selector_output.status.code(), Some(1));
    let selector_report: serde_json::Value =
        serde_json::from_slice(&selector_output.stdout).expect("parse selector outdated JSON");
    assert_eq!(selector_report["pnpm"]["current"], "10.0.0");
    assert_eq!(selector_report["pnpm"]["latest"], "11.0.0");
}

#[test]
fn outdated_reports_cargo_and_python_dependencies() {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    let cache_dir = root.path().join("cache");
    fs::create_dir(&workspace).unwrap();

    // Populate Cargo sparse-index cache for crate `foo` with 1.0.0, 1.2.0, and 2.0.0.
    let cargo_index_dir = cache_dir.join("v11/cargo-index/crates-io/3/f");
    fs::create_dir_all(&cargo_index_dir).unwrap();
    let cargo_index_entry = ["1.0.0", "1.2.0", "2.0.0"]
        .iter()
        .map(|version| {
            format!(
                r#"{{"name":"foo","vers":"{version}","deps":[],"cksum":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","features":{{}},"yanked":false}}"#,
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(cargo_index_dir.join("foo"), cargo_index_entry).unwrap();

    // Serve Python Simple JSON index for `requests` with 1.0, 1.5, and 2.0.
    let mut server = mockito::Server::new();
    let _pypi_mock = server
        .mock("GET", "/simple/requests/")
        .match_header("accept", "application/vnd.pypi.simple.v1+json")
        .with_status(200)
        .with_header("content-type", "application/vnd.pypi.simple.v1+json")
        .with_body(
            serde_json::json!({
                "meta": { "api-version": "1.0" },
                "name": "requests",
                "files": [
                    { "filename": "requests-1.0-py3-none-any.whl", "url": "/files/requests-1.0-py3-none-any.whl", "hashes": { "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa" } },
                    { "filename": "requests-1.5-py3-none-any.whl", "url": "/files/requests-1.5-py3-none-any.whl", "hashes": { "sha256": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb" } },
                    { "filename": "requests-2.0-py3-none-any.whl", "url": "/files/requests-2.0-py3-none-any.whl", "hashes": { "sha256": "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc" } },
                ],
            })
            .to_string(),
        )
        .expect_at_least(1)
        .create();

    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        format!(
            "cargo:\n  enabled: true\npython:\n  enabled: true\nregistries:\n  '{}/simple/':\n    ecosystem: pypi\nstoreDir: '{}'\ncacheDir: '{}'\n",
            server.url(),
            root.path().join("store").display(),
            cache_dir.display(),
        ),
    )
    .unwrap();
    fs::write(
        workspace.join("Cargo.toml"),
        "[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\nfoo = \"^1.0.0\"\n",
    )
    .unwrap();
    fs::write(
        workspace.join("Cargo.lock"),
        "version = 4\n\n[[package]]\nname = \"app\"\nversion = \"0.1.0\"\ndependencies = [\"foo\"]\n\n[[package]]\nname = \"foo\"\nversion = \"1.0.0\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\nchecksum = \"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"\n",
    )
    .unwrap();
    fs::write(
        workspace.join("pyproject.toml"),
        "[project]\nname = 'app'\nversion = '1.0'\nrequires-python = '>=3.10'\ndependencies = ['requests>=1.0,<2.0']\n",
    )
    .unwrap();
    fs::write(
        workspace.join("pylock.toml"),
        "lock-version = '1.0'\nenvironments = []\ncreated-by = 'pnpm'\nrequires-python = '>=3.10'\n\n[[packages]]\nname = 'requests'\nversion = '1.0'\nwheels = [{ name = 'requests-1.0-py3-none-any.whl', url = 'https://example.test/requests-1.0-py3-none-any.whl', hashes = { sha256 = 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa' } }]\n",
    )
    .unwrap();

    // Populate Python index cache so `--offline` works for both Cargo and Python, or test online for Python + cached Cargo:
    let py_output = pacquet(&workspace, ["outdated", "pypi:requests", "--format", "json"])
        .output()
        .expect("run outdated pypi:requests --format json");
    assert_eq!(
        py_output.status.code(),
        Some(1),
        "stderr: {}",
        String::from_utf8_lossy(&py_output.stderr),
    );
    let py_report: serde_json::Value = serde_json::from_slice(&py_output.stdout)
        .unwrap_or_else(|err| {
            panic!(
                "parse python outdated JSON: {err}; stdout: {}; stderr: {}",
                String::from_utf8_lossy(&py_output.stdout),
                String::from_utf8_lossy(&py_output.stderr),
            )
        });
    assert_eq!(py_report["requests"]["current"], "1.0");
    assert_eq!(py_report["requests"]["latest"], "2.0");
    assert_eq!(py_report["requests"]["wanted"], "1.5");
    assert!(py_report.get("foo").is_none());

    // Now both Python and Cargo index entries are cached; run `outdated --offline` and `--compatible`:
    let all_output = pacquet(&workspace, ["outdated", "--offline", "--format", "json"])
        .output()
        .expect("run outdated --offline --format json");
    assert_eq!(
        all_output.status.code(),
        Some(1),
        "stderr: {}",
        String::from_utf8_lossy(&all_output.stderr),
    );
    let all_report: serde_json::Value =
        serde_json::from_slice(&all_output.stdout).expect("parse all outdated JSON");
    assert_eq!(all_report["foo"]["current"], "1.0.0");
    assert_eq!(all_report["foo"]["latest"], "2.0.0");
    assert_eq!(all_report["foo"]["wanted"], "1.2.0");
    assert_eq!(all_report["requests"]["current"], "1.0");
    assert_eq!(all_report["requests"]["latest"], "2.0");
    assert_eq!(all_report["requests"]["wanted"], "1.5");

    let compat_output =
        pacquet(&workspace, ["outdated", "--compatible", "--offline", "--format", "json"])
            .output()
            .expect("run outdated --compatible --offline --format json");
    assert_eq!(compat_output.status.code(), Some(1));
    let compat_report: serde_json::Value =
        serde_json::from_slice(&compat_output.stdout).expect("parse compatible outdated JSON");
    assert_eq!(compat_report["foo"]["latest"], "1.2.0");
    assert_eq!(compat_report["requests"]["latest"], "1.5");

    // Add a root package.json and a standalone Python project sharing the same outdated dependency `requests`:
    fs::write(workspace.join("package.json"), r#"{"name":"app","version":"0.1.0"}"#).unwrap();
    let standalone_py = workspace.join("packages/py-standalone");
    fs::create_dir_all(&standalone_py).unwrap();
    fs::write(
        standalone_py.join("pyproject.toml"),
        "[project]\nname = 'py-standalone'\nversion = '1.0'\nrequires-python = '>=3.10'\ndependencies = ['requests>=1.0,<2.0']\n",
    )
    .unwrap();
    fs::write(
        standalone_py.join("pylock.toml"),
        "lock-version = '1.0'\nenvironments = []\ncreated-by = 'pnpm'\nrequires-python = '>=3.10'\n\n[[packages]]\nname = 'requests'\nversion = '1.0'\nwheels = [{ name = 'requests-1.0-py3-none-any.whl', url = 'https://example.test/requests-1.0-py3-none-any.whl', hashes = { sha256 = 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa' } }]\n",
    )
    .unwrap();

    // Recursive `outdated -r` groups both `app` and `py-standalone` under `requests.dependentPackages`:
    let recursive_output =
        pacquet(&workspace, ["-r", "outdated", "pypi:requests", "--offline", "--format", "json"])
            .output()
            .expect("run -r outdated pypi:requests --offline --format json");
    assert_eq!(recursive_output.status.code(), Some(1));
    let recursive_report: serde_json::Value =
        serde_json::from_slice(&recursive_output.stdout).expect("parse recursive outdated JSON");
    let dependents: Vec<&str> = recursive_report["requests"]["dependentPackages"]
        .as_array()
        .expect("dependentPackages array")
        .iter()
        .filter_map(|entry| entry["name"].as_str())
        .collect();
    assert!(dependents.contains(&"app"), "expected app in {dependents:?}");
    assert!(dependents.contains(&"py-standalone"), "expected py-standalone in {dependents:?}");

    // Recursive `--filter py-standalone` selects only the standalone Python project:
    let filtered_output = pacquet(
        &workspace,
        ["-r", "--filter", "py-standalone", "outdated", "--offline", "--format", "json"],
    )
    .output()
    .expect("run -r --filter py-standalone outdated --offline --format json");
    assert_eq!(
        filtered_output.status.code(),
        Some(1),
        "stderr: {}",
        String::from_utf8_lossy(&filtered_output.stderr),
    );
    let filtered_report: serde_json::Value =
        serde_json::from_slice(&filtered_output.stdout).expect("parse filtered outdated JSON");
    let filtered_dependents: Vec<&str> = filtered_report["requests"]["dependentPackages"]
        .as_array()
        .expect("dependentPackages array")
        .iter()
        .filter_map(|entry| entry["name"].as_str())
        .collect();
    assert_eq!(filtered_dependents, vec!["py-standalone"]);
    assert!(filtered_report.get("foo").is_none());
}
