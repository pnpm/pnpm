//! Recursive-publish integration tests that don't touch a registry: they
//! exercise the `publish --recursive` dispatch, `--filter` selection, the
//! private-package skip, the empty-selection no-op, the `--report-summary`
//! output, and the unsupported `--batch` gap. Actually pushing a tarball to a
//! registry is covered separately once the publish-against-pnpr harness lands.

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pacquet_testing_utils::bin::CommandTempCwd;
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

    let summary = fs::read_to_string(workspace.join("pnpm-publish-summary.json"))
        .expect("read pnpm-publish-summary.json");
    let value: Value = serde_json::from_str(&summary).expect("parse publish summary");
    assert_eq!(
        value["publishedPackages"].as_array().expect("publishedPackages is an array").len(),
        0,
        "no package should be published when all are private",
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
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(
        stderr.contains("Batch publishing (--batch) is not yet supported"),
        "expected the batch-unsupported message, got: {stderr}",
    );

    drop(root);
}
