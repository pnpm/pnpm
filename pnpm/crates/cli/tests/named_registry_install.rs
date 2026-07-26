//! Installing a dependency through a `namedRegistries` alias, in both
//! the plain (`work:<range>`) and the aliased (`work:<pkg>@<range>`)
//! shape.

pub mod _utils;

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pacquet_lockfile::{Lockfile, PkgName};
use pacquet_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use pretty_assertions::assert_eq;
use std::{ffi::OsStr, fs, path::Path, process::Command};
use tempfile::TempDir;

/// Points the `work:` named-registry alias at the mocked registry, which
/// serves the `@pnpm.e2e/foo` fixture.
fn setup() -> (TempDir, std::path::PathBuf, AddMockedRegistry) {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let workspace_yaml = workspace.join("pnpm-workspace.yaml");
    let yaml = fs::read_to_string(&workspace_yaml).expect("read the harness pnpm-workspace.yaml");
    let registry = npmrc_info.mock_instance.url();
    fs::write(&workspace_yaml, format!("{yaml}namedRegistries:\n  work: {registry}\n"))
        .expect("write pnpm-workspace.yaml");
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
fn install_records_a_plain_named_registry_dependency() {
    let (root, workspace, anchor) = setup();

    write_manifest(&workspace, r#"{ "@pnpm.e2e/foo": "work:1.0.0" }"#);
    pacquet(&workspace, ["install"]).assert().success();

    assert_eq!(
        lockfile_entry(&workspace, "@pnpm.e2e/foo"),
        Some(("work:1.0.0".to_string(), "1.0.0".to_string())),
    );

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
        Some(("work:@pnpm.e2e/foo@1.0.0".to_string(), "@pnpm.e2e/foo@1.0.0".to_string())),
    );
    assert!(workspace.join("node_modules/foo-from-work/package.json").exists());

    drop((root, anchor));
}
