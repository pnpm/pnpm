//! Installing and updating dependencies through a `namedRegistries` alias, in both
//! the plain (`work:<range>`) and the aliased (`work:<pkg>@<range>`)
//! shape.

use crate::_utils;

use _utils::{
    append_workspace_yaml_key,
    bravo_dep_mature_up_to_1_0_1_minimum_release_age,
    set_minimum_release_age,
};
use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_lockfile::{
    Lockfile,
    PkgName,
};
use pnpm_testing_utils::bin::{
    AddMockedRegistry,
    CommandTempCwd,
};
use pretty_assertions::assert_eq;
use std::{
    ffi::OsStr,
    fs,
    path::Path,
    process::Command,
};
use tempfile::TempDir;

/// Points the `work:` named-registry alias at the mocked registry, which
/// serves the `@pnpm.e2e/foo` fixture.
fn setup() -> (TempDir, std::path::PathBuf, AddMockedRegistry) {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let registry = npmrc_info.mock_instance.url();
    append_workspace_yaml_key(&workspace, "namedRegistries", format!("{{ work: '{registry}' }}"));
    (root, workspace, npmrc_info)
}

fn pacquet(workspace: &Path, args: impl IntoIterator<Item = impl AsRef<OsStr>>) -> Command {
    Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(workspace)
        .with_args(args)
}

fn write_manifest(workspace: &Path, dependencies: &str) {
    let manifest = format!(
        r#"{{ "name": "test-named-registry", "version": "1.0.0", "dependencies": {dependencies} }}"#,
    );
    fs::write(workspace.join("package.json"), manifest).expect("write package.json");
}

/// The stamped `lockfileVersion` of the workspace's lockfile.
fn lockfile_version(workspace: &Path) -> String {
    let text = fs::read_to_string(workspace.join(Lockfile::FILE_NAME)).expect("read lockfile");
    let lockfile: Lockfile = serde_saphyr::from_str(&text)
        .unwrap_or_else(|error| panic!("parse pnpm-lock.yaml: {error}\n{text}"));
    lockfile.lockfile_version.to_string()
}

/// The lockfile's recorded `(specifier, version)` for a root dependency.
fn lockfile_entry(workspace: &Path, alias: &str) -> Option<(String, String)> {
    let text = fs::read_to_string(workspace.join(Lockfile::FILE_NAME)).expect("read lockfile");
    let lockfile: Lockfile = serde_saphyr::from_str(&text)
        .unwrap_or_else(|error| panic!("parse pnpm-lock.yaml: {error}\n{text}"));
    let alias: PkgName = alias.parse().expect("parse alias");
    let entry = lockfile.importers
        .get(Lockfile::ROOT_IMPORTER_KEY)?
        .dependencies
        .as_ref()?
        .get(&alias)?;
    Some((entry.specifier.clone(), entry.version.to_string()))
}

#[test]
fn install_records_a_plain_named_registry_dependency() {
    let (root, workspace, anchor) = setup();

    write_manifest(&workspace, r#"{ "@pnpm.e2e/foo": "work:1.0.0" }"#);
    pacquet(&workspace, ["install"]).assert().success();

    assert_eq!(
        lockfile_entry(&workspace, "@pnpm.e2e/foo"),
        Some(("work:1.0.0".to_string(), "work:1.0.0".to_string())),
    );
    // Registry-qualified keys are additive, so recording one must not move
    // the lockfile format: an older pnpm reading this file gates on the major
    // and would reject anything outside 9.x outright.
    assert_eq!(lockfile_version(&workspace), "9.0");

    drop((root, anchor));
}

/// A lockfile carrying registry-qualified keys has to replay through a
/// frozen install, which reads the keys back rather than re-resolving them.
#[test]
fn frozen_install_replays_a_registry_qualified_lockfile() {
    let (root, workspace, anchor) = setup();

    write_manifest(&workspace, r#"{ "@pnpm.e2e/foo": "work:1.0.0" }"#);
    pacquet(&workspace, ["install"]).assert().success();

    pacquet(&workspace, ["install", "--frozen-lockfile"]).assert().success();
    assert_eq!(
        lockfile_entry(&workspace, "@pnpm.e2e/foo"),
        Some(("work:1.0.0".to_string(), "work:1.0.0".to_string())),
    );
    assert_eq!(lockfile_version(&workspace), "9.0");

    drop((root, anchor));
}

/// The aliased shape resolves under the package name the specifier
/// names, but the importer records it under the manifest's own key.
#[test]
fn install_records_an_aliased_named_registry_dependency() {
    let (root, workspace, anchor) = setup();

    write_manifest(&workspace, r#"{ "foo-from-work": "work:@pnpm.e2e/foo@1.0.0" }"#);
    pacquet(&workspace, ["install"]).assert().success();

    assert_eq!(
        lockfile_entry(&workspace, "foo-from-work"),
        Some(("work:@pnpm.e2e/foo@1.0.0".to_string(), "@pnpm.e2e/foo@work:1.0.0".to_string())),
    );
    assert!(workspace.join("node_modules/foo-from-work/package.json").exists());

    drop((root, anchor));
}

#[test]
fn strict_peers_accept_named_registry_versions_on_fresh_and_frozen_installs() {
    let (root, workspace, anchor) = setup();
    append_workspace_yaml_key(&workspace, "strictPeerDependencies", "true");
    write_manifest(
        &workspace,
        r#"{
            "@pnpm.e2e/has-foo100-peer": "work:1.0.0",
            "@pnpm.e2e/foo": "work:100.0.0"
        }"#,
    );
    pacquet(&workspace, ["install"]).assert().success();
    pacquet(&workspace, ["install", "--frozen-lockfile"]).assert().success();
    pacquet(&workspace, ["peers", "check"]).assert().success();
    assert_eq!(
        lockfile_entry(&workspace, "@pnpm.e2e/foo"),
        Some(("work:100.0.0".to_string(), "work:100.0.0".to_string())),
    );
    drop((root, anchor));
}

#[test]
fn outdated_and_update_latest_use_the_named_registry() {
    let (root, workspace, anchor) = setup();
    let name = "@pnpm.e2e/dep-of-pkg-with-1-dep";
    write_manifest(
        &workspace,
        &format!(r#"{{ "{name}": "work:100.0.0", "aliased": "work:{name}@100.0.0" }}"#),
    );
    pacquet(&workspace, ["install"]).assert().success();

    for selector in [name, "aliased"] {
        let output = pacquet(&workspace, ["outdated", "--format", "json", selector])
            .output()
            .expect("run outdated");
        assert_eq!(output.status.code(), Some(1), "{output:?}");
        let report: serde_json::Value =
            serde_json::from_slice(&output.stdout).expect("parse report");
        assert_eq!(report[name]["current"], "100.0.0");
        assert_eq!(report[name]["latest"], "101.0.0");
    }

    pacquet(&workspace, ["outdated", "--compatible", "--format", "json"])
        .assert()
        .success()
        .stdout("{}\n");
    pacquet(&workspace, ["update", "--latest"]).assert().success();
    assert_eq!(
        lockfile_entry(&workspace, name),
        Some(("work:101.0.0".to_string(), "work:101.0.0".to_string())),
    );
    assert_eq!(
        lockfile_entry(&workspace, "aliased"),
        Some((format!("work:{name}@101.0.0"), format!("{name}@work:101.0.0"))),
    );
    drop((root, anchor));
}

#[test]
fn outdated_named_registry_catalog_respects_release_age_and_exclusions() {
    let (root, workspace, anchor) = setup();
    let name = "@pnpm.e2e/bravo-dep";
    append_workspace_yaml_key(&workspace, "catalog", format!("{{ '{name}': 'work:1.0.0' }}"));
    write_manifest(&workspace, &format!(r#"{{ "{name}": "catalog:" }}"#));
    set_minimum_release_age(&workspace, bravo_dep_mature_up_to_1_0_1_minimum_release_age());
    pacquet(&workspace, ["install"]).assert().success();

    for (excludes, expected) in [(None, "1.0.1"), (Some("['@pnpm.e2e/*']"), "1.1.0")] {
        if let Some(excludes) = excludes {
            append_workspace_yaml_key(&workspace, "minimumReleaseAgeExclude", excludes);
        }
        let output = pacquet(
            &workspace,
            ["outdated", "--format", "json", "--registry=http://127.0.0.1:9/", name],
        )
        .output()
        .expect("run outdated");
        assert_eq!(output.status.code(), Some(1), "{output:?}");
        let report: serde_json::Value =
            serde_json::from_slice(&output.stdout).expect("parse report");
        assert_eq!(report[name]["current"], "1.0.0");
        assert_eq!(report[name]["latest"], expected);
    }
    drop((root, anchor));
}
