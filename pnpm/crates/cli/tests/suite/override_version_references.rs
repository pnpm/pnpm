//! `$dep-name` self-references in `overrides` end to end: the reference
//! resolves against the root manifest's direct dependencies, and the
//! resolved specifier — not the raw `$dep-name` — is what the lockfile
//! records and what a later frozen install compares against.

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_lockfile::Lockfile;
use pnpm_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use pretty_assertions::assert_eq;
use std::{fs, path::Path, process::Command};

const DEP: &str = "@pnpm.e2e/dep-of-pkg-with-1-dep";

/// Append an `overrides` block to the `pnpm-workspace.yaml` the mocked
/// registry already wrote (it carries `storeDir`/`cacheDir`, so the
/// tests must extend it rather than overwrite it).
fn add_overrides(workspace: &Path, block: &str) {
    let path = workspace.join("pnpm-workspace.yaml");
    let mut yaml = fs::read_to_string(&path).unwrap_or_default();
    if !yaml.is_empty() && !yaml.ends_with('\n') {
        yaml.push('\n');
    }
    yaml.push_str(block);
    fs::write(&path, yaml).expect("update pnpm-workspace.yaml");
}

fn write_manifest(workspace: &Path, dep_spec: &str) {
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "name": "override-self-reference",
            "version": "1.0.0",
            "dependencies": { DEP: dep_spec },
        })
        .to_string(),
    )
    .expect("write package.json");
}

fn lockfile_overrides(workspace: &Path) -> Vec<(String, String)> {
    let text = fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read pnpm-lock.yaml");
    let lockfile: Lockfile = serde_saphyr::from_str(&text).expect("parse pnpm-lock.yaml");
    lockfile
        .overrides
        .iter()
        .flatten()
        .map(|(selector, spec)| (selector.clone(), spec.clone()))
        .collect()
}

#[test]
fn install_resolves_a_reference_and_a_frozen_install_accepts_the_lockfile() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_manifest(&workspace, "^100.0.0");
    add_overrides(&workspace, &format!("overrides:\n  \"{DEP}\": ${DEP}\n"));

    let output = pacquet.with_arg("install").assert().success();
    let stdout = String::from_utf8_lossy(&output.get_output().stdout).into_owned();
    eprintln!("STDOUT:\n{stdout}\n");

    assert_eq!(lockfile_overrides(&workspace), vec![(DEP.to_string(), "^100.0.0".to_string())]);
    assert!(
        stdout.contains(&format!(
            "The \"$\" version reference syntax in overrides is deprecated (used by: {DEP}). \
             Define the version in a catalog and reference it with the \"catalog:\" protocol \
             instead. See https://pnpm.io/catalogs"
        )),
        "{stdout}",
    );

    // The freshness check compares the lockfile's `overrides` against
    // the configured ones, so an unresolved `$dep-name` here would fail
    // the install as outdated.
    Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(&workspace)
        .with_args(["install", "--frozen-lockfile"])
        .assert()
        .success();

    drop((root, mock_instance));
}

#[test]
fn install_rejects_a_reference_to_a_package_that_is_not_a_direct_dependency() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_manifest(&workspace, "^100.0.0");
    add_overrides(&workspace, &format!("overrides:\n  \"{DEP}\": $is-odd\n"));

    let output = pacquet.with_arg("install").assert().failure();
    let stderr = String::from_utf8_lossy(&output.get_output().stderr).into_owned();
    eprintln!("STDERR:\n{stderr}\n");

    assert!(stderr.contains("ERR_PNPM_CANNOT_RESOLVE_OVERRIDE_VERSION"), "{stderr}");
    // miette wraps the message across terminal-width lines, so compare
    // against a whitespace-collapsed rendering.
    let unwrapped = stderr.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(
        unwrapped.contains(
            r#"Cannot resolve version $is-odd in overrides. The direct dependencies don't have dependency "is-odd"."#
        ),
        "{stderr}",
    );

    drop((root, mock_instance));
}
