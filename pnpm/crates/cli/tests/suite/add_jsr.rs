//! `pacquet add` over `jsr:` selectors, the shape reported in
//! [pnpm/pnpm#14590](https://github.com/pnpm/pnpm/issues/14590).

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_lockfile::{Lockfile, PkgName};
use pnpm_package_manifest::{DependencyGroup, PackageManifest};
use pnpm_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use pretty_assertions::assert_eq;
use std::{ffi::OsStr, fs, path::Path, process::Command};
use tempfile::TempDir;

/// `jsr:` specifiers resolve through the `@jsr` scope, which defaults to
/// `npm.jsr.io`; point it at the mocked registry, which serves the
/// `@jsr/pnpm-e2e__bar` fixture up to 2.0.0.
fn setup() -> (TempDir, std::path::PathBuf, AddMockedRegistry) {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let npmrc = fs::read_to_string(&npmrc_info.npmrc_path).expect("read the harness .npmrc");
    let jsr_registry = npmrc_info.mock_instance.url();
    fs::write(&npmrc_info.npmrc_path, format!("{npmrc}@jsr:registry={jsr_registry}\n"))
        .expect("write .npmrc");
    fs::write(workspace.join("package.json"), r#"{ "name": "test-add-jsr", "version": "1.0.0" }"#)
        .expect("write package.json");
    (root, workspace, npmrc_info)
}

fn pacquet(workspace: &Path, args: impl IntoIterator<Item = impl AsRef<OsStr>>) -> Command {
    Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(workspace)
        .with_args(args)
}

fn dep_spec(workspace: &Path, name: &str) -> Option<String> {
    let manifest = PackageManifest::from_path(workspace.join("package.json")).unwrap();
    manifest
        .dependencies([DependencyGroup::Prod])
        .find(|(key, _)| *key == name)
        .map(|(_, spec)| spec.to_string())
}

/// The lockfile's recorded `(specifier, version)` for a root dependency.
fn lockfile_entry(workspace: &Path, alias: &str) -> Option<(String, String)> {
    let text = fs::read_to_string(workspace.join(Lockfile::FILE_NAME)).expect("read lockfile");
    let lockfile: Lockfile = serde_saphyr::from_str(&text)
        .unwrap_or_else(|error| panic!("parse pnpm-lock.yaml: {error}\n{text}"));
    let alias: PkgName = alias.parse().expect("parse alias");
    let entry =
        lockfile.importers.get(Lockfile::ROOT_IMPORTER_KEY)?.dependencies.as_ref()?.get(&alias)?;
    Some((entry.specifier.clone(), entry.version.to_string()))
}

#[test]
fn add_saves_a_jsr_selector_under_its_jsr_name() {
    let (root, workspace, anchor) = setup();

    pacquet(&workspace, ["add", "jsr:@pnpm-e2e/bar"]).assert().success();

    assert_eq!(dep_spec(&workspace, "@pnpm-e2e/bar").as_deref(), Some("jsr:^2.0.0"));
    assert_eq!(
        lockfile_entry(&workspace, "@pnpm-e2e/bar"),
        Some(("jsr:^2.0.0".to_string(), "@jsr/pnpm-e2e__bar@2.0.0".to_string())),
    );
    assert!(workspace.join("node_modules/@pnpm-e2e/bar/package.json").exists());

    drop((root, anchor));
}

/// A version selector pins the picked version with the operator it asks
/// for, the same way a plain registry range does.
#[test]
fn add_keeps_the_range_operator_a_jsr_selector_asks_for() {
    let (root, workspace, anchor) = setup();

    pacquet(&workspace, ["add", "jsr:@pnpm-e2e/bar@1.0"]).assert().success();

    assert_eq!(dep_spec(&workspace, "@pnpm-e2e/bar").as_deref(), Some("jsr:~1.0.1"));
    assert_eq!(
        lockfile_entry(&workspace, "@pnpm-e2e/bar"),
        Some(("jsr:~1.0.1".to_string(), "@jsr/pnpm-e2e__bar@1.0.1".to_string())),
    );

    drop((root, anchor));
}
