//! Recursive-publish integration tests. The no-registry tests exercise the
//! `publish --recursive` dispatch, `--filter` selection, the private-package
//! skip, the empty-selection no-op, the `--report-summary` output, and the
//! unsupported `--batch` gap. The registry tests drive the real binary against
//! a `mockito` registry (pnpr's `TestRegistry` is proxy-mode and rejects
//! path-less publishes) to cover the actual publish loop: the not-yet-published
//! probe, the per-package `PUT`, `--force`, and the summary / `--json` shapes —
//! porting the plain token-auth scenarios from pnpm's `recursivePublish.ts`.

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use mockito::Matcher;
use pacquet_testing_utils::bin::CommandTempCwd;
use pipe_trait::Pipe;
use serde_json::{Value, json};
use std::{fs, path::Path};

/// Write a `pnpm-workspace.yaml` listing `names` as packages, plus a
/// `package.json` per name under its own subdirectory of `workspace`.
fn write_workspace(workspace: &Path, manifests: &[(&str, Value)]) {
    let packages = manifests.iter().map(|(name, _)| format!("  - {name}")).collect::<Vec<_>>();
    let workspace_yaml = format!("packages:\n{}\n", packages.join("\n"));
    fs::write(workspace.join("pnpm-workspace.yaml"), workspace_yaml)
        .expect("write pnpm-workspace.yaml");
    for (name, manifest) in manifests {
        let dir = workspace.join(name);
        fs::create_dir_all(&dir).expect("create project dir");
        fs::write(dir.join("package.json"), manifest.to_string()).expect("write package.json");
    }
}

fn private_pkg(name: &str) -> Value {
    json!({ "name": name, "version": "1.0.0", "private": true })
}

fn public_pkg(name: &str) -> Value {
    json!({ "name": name, "version": "1.0.0" })
}

/// Point the workspace at `registry` for publishing.
fn write_registry_npmrc(workspace: &Path, registry: &str) {
    fs::write(workspace.join(".npmrc"), format!("registry={registry}\n")).expect("write .npmrc");
}

/// Clear the CI / OIDC environment so the spawned publish never attempts an
/// id-token exchange and stays offline against the mocked registry.
fn clear_ci<Command: CommandExtra>(command: Command) -> Command {
    command
        .without_env("GITHUB_ACTIONS")
        .without_env("GITLAB_CI")
        .without_env("NPM_ID_TOKEN")
        .without_env("ACTIONS_ID_TOKEN_REQUEST_TOKEN")
        .without_env("ACTIONS_ID_TOKEN_REQUEST_URL")
}

/// A `--filter` that matches no project narrows the workspace to nothing, so
/// recursive publish exits 0 without publishing or writing a summary — an
/// empty selection is a no-op.
#[test]
fn recursive_publish_filter_no_match_is_a_noop() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace(
        &workspace,
        &[("project-1", private_pkg("project-1")), ("project-2", private_pkg("project-2"))],
    );

    pacquet
        .with_arg("-r")
        .with_arg("publish")
        .with_arg("--filter=does-not-exist")
        .with_arg("--no-git-checks")
        .assert()
        .success();

    assert!(
        !workspace.join("pnpm-publish-summary.json").exists(),
        "an empty selection must not write a publish summary",
    );

    drop(root);
}

/// When every selected package is private there is nothing to publish, so the
/// command succeeds and `--report-summary` records an empty `publishedPackages`
/// list. No registry request is made because private packages are filtered out
/// before the already-published check.
#[test]
fn recursive_publish_all_private_writes_empty_summary() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace(
        &workspace,
        &[("project-1", private_pkg("project-1")), ("project-2", private_pkg("project-2"))],
    );

    pacquet
        .with_arg("-r")
        .with_arg("publish")
        .with_arg("--report-summary")
        .with_arg("--no-git-checks")
        .assert()
        .success();

    let summary = workspace
        .join("pnpm-publish-summary.json")
        .pipe(fs::read_to_string)
        .expect("read pnpm-publish-summary.json");
    let value: Value = serde_json::from_str(&summary).expect("parse publish summary");
    assert_eq!(
        value["publishedPackages"].as_array().expect("publishedPackages is an array").len(),
        0,
        "no package should be published when all are private",
    );

    drop(root);
}

/// `publish -r --json` prints the per-package summaries as a JSON array on
/// stdout — an empty array when nothing is published. Exercised on the
/// all-private no-op path so no registry request is made.
#[test]
fn recursive_publish_json_prints_empty_array_when_nothing_published() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace(
        &workspace,
        &[("project-1", private_pkg("project-1")), ("project-2", private_pkg("project-2"))],
    );

    let assert = pacquet
        .with_arg("-r")
        .with_arg("publish")
        .with_arg("--json")
        .with_arg("--no-git-checks")
        .assert()
        .success();
    // The global "no new packages" info line shares stdout with the reporter
    // (matching pnpm's `logger.info`), so assert on the JSON array line itself
    // rather than the whole stream: it is present only because `--json` prints
    // `publishedPackages`, and disappears if that print is dropped.
    let stdout = assert.get_output().stdout.pipe_as_ref(String::from_utf8_lossy);
    assert!(
        stdout.lines().any(|line| line.trim() == "[]"),
        "recursive --json must print the published-packages array (empty here) on stdout, got: {stdout:?}",
    );

    drop(root);
}

/// `--batch` is accepted for surface parity but not yet ported, so a recursive
/// batch publish fails fast with an explicit message rather than silently
/// publishing per-package.
#[test]
fn recursive_publish_batch_is_unsupported() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace(&workspace, &[("project-1", private_pkg("project-1"))]);

    let assert = pacquet
        .with_arg("-r")
        .with_arg("publish")
        .with_arg("--batch")
        .with_arg("--no-git-checks")
        .assert()
        .failure();
    let stderr = assert.get_output().stderr.pipe_as_ref(String::from_utf8_lossy);
    assert!(
        stderr.contains("Batch publishing (--batch) is not yet supported"),
        "expected the batch-unsupported message, got: {stderr}",
    );

    drop(root);
}

/// A bare `--filter` (no `-r`) puts `publish` into recursive mode — the shape
/// `release.yml` drives
/// publishing with (`pn publish --filter=<pkg>`). A filter that matches no
/// project is then a recursive no-op (exit 0); without the promotion this
/// would fall through to the single-package path and fail for lack of a
/// `package.json` in the workspace-root cwd.
#[test]
fn filter_without_recursive_flag_enters_recursive_publish() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace(
        &workspace,
        &[("project-1", private_pkg("project-1")), ("project-2", private_pkg("project-2"))],
    );

    pacquet
        .with_arg("publish")
        .with_arg("--filter=does-not-exist")
        .with_arg("--no-git-checks")
        .assert()
        .success();

    assert!(
        !workspace.join("pnpm-publish-summary.json").exists(),
        "an empty selection must not write a publish summary",
    );

    drop(root);
}

/// The exact shape `release.yml` publishes the monorepo with — an exclusion
/// selector and no `-r` (`pn publish --filter=!<pkg> ...`). The promotion
/// enters recursive mode, the exclusion narrows the set, and because every
/// remaining package is private the run is a successful no-op that records an
/// empty `publishedPackages` list.
#[test]
fn filter_exclusion_without_recursive_flag_publishes_nothing() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace(
        &workspace,
        &[("project-1", private_pkg("project-1")), ("project-2", private_pkg("project-2"))],
    );

    pacquet
        .with_arg("publish")
        .with_arg("--filter=!project-1")
        .with_arg("--report-summary")
        .with_arg("--no-git-checks")
        .assert()
        .success();

    let summary = workspace
        .join("pnpm-publish-summary.json")
        .pipe(fs::read_to_string)
        .expect("read pnpm-publish-summary.json");
    let value: Value = serde_json::from_str(&summary).expect("parse publish summary");
    assert_eq!(
        value["publishedPackages"].as_array().expect("publishedPackages is an array").len(),
        0,
        "every selected package is private, so nothing is published",
    );

    drop(root);
}

/// The global `-r` short flag works *after* the `publish` subcommand, not only
/// before it — `pnpm publish -r` is the canonical recursive-publish ordering.
/// (`publish` must not declare its own `--recursive` arg, which would strip the
/// global `-r` short from the subcommand.)
#[test]
fn recursive_publish_short_flag_after_subcommand() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace(
        &workspace,
        &[("project-1", private_pkg("project-1")), ("project-2", private_pkg("project-2"))],
    );

    pacquet.with_arg("publish").with_arg("-r").with_arg("--no-git-checks").assert().success();

    drop(root);
}

/// A workspace that enumerates no project is a recursive no-op that writes no
/// summary — publishing returns before the handler when there are no
/// projects, so no empty `pnpm-publish-summary.json` is emitted.
#[test]
fn recursive_publish_empty_workspace_writes_no_summary() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    fs::write(workspace.join("pnpm-workspace.yaml"), "packages: []\n")
        .expect("write pnpm-workspace.yaml");

    pacquet
        .with_arg("publish")
        .with_arg("-r")
        .with_arg("--report-summary")
        .with_arg("--no-git-checks")
        .assert()
        .success();

    assert!(
        !workspace.join("pnpm-publish-summary.json").exists(),
        "an empty workspace must not write a publish summary",
    );

    drop(root);
}

/// Each eligible workspace package is probed (a 404 means "not yet published")
/// and then pushed with its own `PUT`.
#[test]
fn recursive_publish_pushes_each_eligible_package() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    write_workspace(
        &workspace,
        &[("project-1", public_pkg("project-1")), ("project-2", public_pkg("project-2"))],
    );
    write_registry_npmrc(&workspace, &format!("{}/", server.url()));

    let probe_1 = server.mock("GET", "/project-1").with_status(404).create();
    let probe_2 = server.mock("GET", "/project-2").with_status(404).create();
    let put_1 =
        server.mock("PUT", "/project-1").with_status(200).with_body("{}").expect(1).create();
    let put_2 =
        server.mock("PUT", "/project-2").with_status(200).with_body("{}").expect(1).create();

    clear_ci(pacquet)
        .with_arg("-r")
        .with_arg("publish")
        .with_arg("--no-git-checks")
        .assert()
        .success();

    probe_1.assert();
    probe_2.assert();
    put_1.assert();
    put_2.assert();
    drop(root);
}

/// A package whose current version is already on the registry is skipped; only
/// the not-yet-published package is pushed.
#[test]
fn recursive_publish_skips_already_published_packages() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    write_workspace(
        &workspace,
        &[("project-1", public_pkg("project-1")), ("project-2", public_pkg("project-2"))],
    );
    write_registry_npmrc(&workspace, &format!("{}/", server.url()));

    // project-1 already has 1.0.0 published (an abbreviated packument that lists
    // the version), so it is skipped; project-2 is absent (404) and published.
    let packument = json!({
        "name": "project-1",
        "dist-tags": { "latest": "1.0.0" },
        "versions": {
            "1.0.0": {
                "name": "project-1",
                "version": "1.0.0",
                "dist": {
                    "tarball": "http://example.test/project-1-1.0.0.tgz",
                    "integrity": "sha512-deadbeef",
                },
            },
        },
    });
    server.mock("GET", "/project-1").with_status(200).with_body(packument.to_string()).create();
    server.mock("GET", "/project-2").with_status(404).create();
    let put_1 = server.mock("PUT", "/project-1").expect(0).create();
    let put_2 =
        server.mock("PUT", "/project-2").with_status(200).with_body("{}").expect(1).create();

    clear_ci(pacquet)
        .with_arg("-r")
        .with_arg("publish")
        .with_arg("--no-git-checks")
        .assert()
        .success();

    put_1.assert();
    put_2.assert();
    drop(root);
}

/// `--force` skips the already-published probe entirely and republishes, so no
/// `GET` is issued.
#[test]
fn recursive_publish_force_republishes_without_probing() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    write_workspace(&workspace, &[("project-1", public_pkg("project-1"))]);
    write_registry_npmrc(&workspace, &format!("{}/", server.url()));

    let probe = server.mock("GET", Matcher::Any).expect(0).create();
    let put = server.mock("PUT", "/project-1").with_status(200).with_body("{}").expect(1).create();

    clear_ci(pacquet)
        .with_arg("-r")
        .with_arg("publish")
        .with_arg("--force")
        .with_arg("--no-git-checks")
        .assert()
        .success();

    probe.assert();
    put.assert();
    drop(root);
}

/// `--report-summary` records every published package under `publishedPackages`
/// with the per-package summary shape (`name` / `version`).
#[test]
fn recursive_publish_report_summary_lists_the_published_packages() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    write_workspace(
        &workspace,
        &[("project-1", public_pkg("project-1")), ("project-2", public_pkg("project-2"))],
    );
    write_registry_npmrc(&workspace, &format!("{}/", server.url()));

    server.mock("GET", Matcher::Any).with_status(404).create();
    server.mock("PUT", Matcher::Any).with_status(200).with_body("{}").create();

    clear_ci(pacquet)
        .with_arg("-r")
        .with_arg("publish")
        .with_arg("--report-summary")
        .with_arg("--no-git-checks")
        .assert()
        .success();

    let summary = workspace
        .join("pnpm-publish-summary.json")
        .pipe(fs::read_to_string)
        .expect("read pnpm-publish-summary.json");
    let value: Value = serde_json::from_str(&summary).expect("parse publish summary");
    let published = value["publishedPackages"].as_array().expect("publishedPackages is an array");
    assert_eq!(published.len(), 2, "both packages should be recorded");
    let mut names: Vec<&str> =
        published.iter().map(|entry| entry["name"].as_str().expect("name")).collect();
    names.sort_unstable();
    assert_eq!(names, ["project-1", "project-2"]);
    assert_eq!(published[0]["version"], "1.0.0");

    drop(root);
}

/// `--json` prints the array of per-package summaries on stdout.
#[test]
fn recursive_publish_json_prints_the_published_array() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    write_workspace(
        &workspace,
        &[("project-1", public_pkg("project-1")), ("project-2", public_pkg("project-2"))],
    );
    write_registry_npmrc(&workspace, &format!("{}/", server.url()));

    server.mock("GET", Matcher::Any).with_status(404).create();
    server.mock("PUT", Matcher::Any).with_status(200).with_body("{}").create();

    let assert = clear_ci(pacquet)
        .with_arg("-r")
        .with_arg("publish")
        .with_arg("--json")
        .with_arg("--no-git-checks")
        .assert()
        .success();

    let stdout = assert.get_output().stdout.pipe_as_ref(String::from_utf8_lossy);
    assert!(
        stdout.contains(r#""id": "project-1@1.0.0""#)
            && stdout.contains(r#""id": "project-2@1.0.0""#),
        "--json must print both per-package summaries, got: {stdout}",
    );

    drop(root);
}
