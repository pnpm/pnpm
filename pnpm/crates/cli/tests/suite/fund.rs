//! `pnpm fund` — the funding information of the installed packages.
//!
//! Opening a funding URL is covered by the unit tests in
//! `src/cli_args/fund/tests.rs` through the `OpenUrl` seam: running it
//! against the real binary would launch the developer's browser.

use crate::_utils::pacquet_in;
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use serde_json::{Value, json};
use std::{fs, path::Path, process::Command};

fn write_manifest(dir: &Path, manifest: &Value) {
    fs::create_dir_all(dir).expect("create package directory");
    fs::write(dir.join("package.json"), manifest.to_string()).expect("write package.json");
}

/// `file:` directory dependencies install offline: `a` and `c` share a
/// funding URL, and `b`, a dependency of `a`, lists two sources.
fn write_funded_packages(dir: &Path) {
    write_manifest(
        &dir.join("a"),
        &json!({
            "name": "a",
            "version": "1.0.0",
            "funding": "https://example.com/shared",
            "dependencies": { "b": "file:../b" },
        }),
    );
    write_manifest(
        &dir.join("b"),
        &json!({
            "name": "b",
            "version": "2.0.0",
            "funding": [{ "type": "github", "url": "https://github.com/sponsors/b" }, "https://example.com/b"],
        }),
    );
    write_manifest(
        &dir.join("c"),
        &json!({
            "name": "c",
            "version": "3.0.0",
            "funding": { "type": "opencollective", "url": "https://example.com/shared" },
        }),
    );
}

fn run(pacquet: Command, args: &[&str]) -> String {
    let output = pacquet
        .with_args(args)
        .output()
        .expect("run pnpm");
    assert!(
        output.status.success(),
        "pnpm {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr),
    );
    String::from_utf8(output.stdout).expect("stdout is UTF-8")
}

fn installed_project() -> (CommandTempCwd<AddMockedRegistry>, impl Fn() -> Command) {
    let env = CommandTempCwd::init().add_mocked_registry();
    write_funded_packages(&env.workspace);
    write_manifest(
        &env.workspace,
        &json!({
            "name": "root",
            "version": "1.0.0",
            "dependencies": { "a": "file:./a" },
            "devDependencies": { "c": "file:./c" },
        }),
    );
    let workspace = env.workspace.clone();
    let pacquet = move || pacquet_in(&workspace);
    run(pacquet(), &["install"]);
    (env, pacquet)
}

#[test]
fn fund_prints_the_npm_fund_tree() {
    let (env, pacquet) = installed_project();

    let expected = [
        "root@1.0.0",
        "└─┬ https://example.com/shared",
        "  │ └── a@1.0.0, c@3.0.0",
        "  └── https://github.com/sponsors/b",
        "      └── b@2.0.0",
        "",
    ];
    assert_eq!(run(pacquet(), &["fund"]), expected.join("\n"));

    drop(env);
}

#[test]
fn fund_json_prints_the_npm_fund_report() {
    let (env, pacquet) = installed_project();

    let report: Value =
        serde_json::from_str(&run(pacquet(), &["fund", "--json"])).expect("parse fund JSON");
    assert_eq!(
        report,
        json!({
            "length": 3,
            "name": "root",
            "version": "1.0.0",
            "dependencies": {
                "a": {
                    "version": "1.0.0",
                    "funding": { "url": "https://example.com/shared" },
                    "dependencies": {
                        "b": {
                            "version": "2.0.0",
                            "funding": [
                                { "type": "github", "url": "https://github.com/sponsors/b" },
                                { "url": "https://example.com/b" },
                            ],
                        },
                    },
                },
                "c": {
                    "version": "3.0.0",
                    "funding": { "type": "opencollective", "url": "https://example.com/shared" },
                },
            },
        }),
    );

    drop(env);
}

#[test]
fn fund_lists_the_sources_of_a_package_with_several() {
    let (env, pacquet) = installed_project();

    let expected = [
        "1: github funding available at the following URL: https://github.com/sponsors/b",
        "2: Funding available at the following URL: https://example.com/b",
        "Run `pnpm fund b --which=1`, for example, to open the first funding URL listed in that package",
        "",
    ];
    assert_eq!(run(pacquet(), &["fund", "b"]), expected.join("\n"));

    drop(env);
}

#[test]
fn fund_fails_for_a_package_without_funding() {
    let (env, pacquet) = installed_project();

    let output = pacquet()
        .with_args(["fund", "missing"])
        .output()
        .expect("run pnpm fund");
    assert!(!output.status.success(), "fund must fail for a package without funding");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("ERR_PNPM_NO_FUNDING"), "stderr: {stderr}");
    assert!(stderr.contains("No valid funding method available for: missing"), "stderr: {stderr}");

    drop(env);
}

/// Two workspace projects, `first` depending on `c` and `second` on `b`,
/// installed with the extra `pnpm-workspace.yaml` settings.
fn installed_workspace(settings: &str) -> CommandTempCwd<AddMockedRegistry> {
    let env = CommandTempCwd::init().add_mocked_registry();
    let workspace = &env.workspace;
    let yaml = fs::read_to_string(workspace.join("pnpm-workspace.yaml")).unwrap_or_default();
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        format!("{yaml}{settings}packages:\n  - projects/*\n"),
    )
    .expect("write pnpm-workspace.yaml");
    write_funded_packages(&workspace.join("libs"));
    write_manifest(workspace, &json!({ "name": "root", "private": true }));
    write_manifest(
        &workspace.join("projects/first"),
        &json!({ "name": "first", "version": "1.0.0", "dependencies": { "c": "file:../../libs/c" } }),
    );
    write_manifest(
        &workspace.join("projects/second"),
        &json!({ "name": "second", "version": "2.0.0", "dependencies": { "b": "file:../../libs/b" } }),
    );
    run(pacquet_in(workspace), &["install"]);
    env
}

/// Each report's project name and the names of its funded dependencies.
fn recursive_summary(workspace: &Path, args: &[&str]) -> Vec<(String, Vec<String>)> {
    let reports: Value =
        serde_json::from_str(&run(pacquet_in(workspace), args)).expect("parse fund JSON");
    reports
        .as_array()
        .expect("a recursive report is an array")
        .iter()
        .map(|report| {
            let dependencies = report["dependencies"].as_object().expect("dependencies object");
            (
                report["name"]
                    .as_str()
                    .expect("project name")
                    .to_string(),
                dependencies.keys().cloned().collect(),
            )
        })
        .collect()
}

fn summary(entries: &[(&str, &[&str])]) -> Vec<(String, Vec<String>)> {
    entries
        .iter()
        .map(|(name, dependencies)| {
            (
                name.to_string(),
                dependencies
                    .iter()
                    .map(ToString::to_string)
                    .collect(),
            )
        })
        .collect()
}

#[test]
fn fund_recursive_reports_every_selected_project() {
    let env = installed_workspace("");
    let workspace = &env.workspace;

    assert_eq!(
        recursive_summary(workspace, &["--recursive", "fund", "--json"]),
        summary(&[("root", &[]), ("first", &["c"]), ("second", &["b"])]),
    );
    assert_eq!(
        recursive_summary(workspace, &["--filter", "second", "fund", "--json"]),
        summary(&[("second", &["b"])]),
    );
    let expected = ["first@1.0.0", "└── https://example.com/shared", "    └── c@3.0.0", ""];
    assert_eq!(run(pacquet_in(&workspace.join("projects/first")), &["fund"]), expected.join("\n"));

    drop(env);
}

#[test]
fn fund_recursive_reads_dedicated_lockfiles() {
    let env = installed_workspace("sharedWorkspaceLockfile: false\n");
    let workspace = &env.workspace;
    assert!(workspace.join("projects/first/pnpm-lock.yaml").exists(), "first has its own lockfile");

    assert_eq!(
        recursive_summary(workspace, &["--recursive", "fund", "--json"]),
        summary(&[("root", &[]), ("first", &["c"]), ("second", &["b"])]),
    );

    drop(env);
}
