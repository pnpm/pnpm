//! Recursive-publish integration tests that don't touch a registry: they
//! exercise the `publish --recursive` dispatch, `--filter` selection, the
//! private-package skip, the empty-selection no-op, the `--report-summary`
//! output, and the unsupported `--batch` gap. Actually pushing a tarball to a
//! registry is covered separately once the publish-against-pnpr harness lands.

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
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

/// A `--filter` that matches no project narrows the workspace to nothing, so
/// recursive publish exits 0 without publishing or writing a summary —
/// matching pnpm's empty-`selectedProjectsGraph` no-op.
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
/// stdout — an empty array when nothing is published — mirroring pnpm's
/// `JSON.stringify(publishedPackages)` for the recursive path. Exercised on the
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

/// A bare `--filter` (no `-r`) puts `publish` into recursive mode, matching
/// pnpm's `parse-cli-args` promotion — the shape `release.yml` drives
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
