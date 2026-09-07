//! `pacquet add` over `jsr:` selectors, the shape reported in
//! [pnpm/pnpm#14590](https://github.com/pnpm/pnpm/issues/14590).

use crate::_utils;

use _utils::{dependency_spec, pacquet_in, read_lockfile};
use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_lockfile::{Lockfile, PkgName, ResolvedDependencySpec};
use pnpm_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use pretty_assertions::assert_eq;
use std::fs;
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

fn root_dependency<'a>(lockfile: &'a Lockfile, alias: &str) -> &'a ResolvedDependencySpec {
    let alias: PkgName = alias.parse().expect("parse alias");
    lockfile
        .importers
        .get(Lockfile::ROOT_IMPORTER_KEY)
        .expect("root importer")
        .dependencies
        .as_ref()
        .expect("root dependencies")
        .get(&alias)
        .expect("the added dependency")
}

#[test]
fn add_saves_a_jsr_selector_under_its_jsr_name() {
    let (root, workspace, anchor) = setup();

    pacquet_in(&workspace).with_args(["add", "jsr:@pnpm-e2e/bar"]).assert().success();

    assert_eq!(
        dependency_spec(&workspace, "dependencies", "@pnpm-e2e/bar").as_deref(),
        Some("jsr:^2.0.0"),
    );
    let lockfile = read_lockfile(&workspace.join(Lockfile::FILE_NAME));
    let entry = root_dependency(&lockfile, "@pnpm-e2e/bar");
    assert_eq!(entry.specifier, "jsr:^2.0.0");
    assert_eq!(entry.version.to_string(), "@jsr/pnpm-e2e__bar@2.0.0");
    assert!(workspace.join("node_modules/@pnpm-e2e/bar/package.json").exists());

    drop((root, anchor));
}

#[test]
fn add_keeps_the_range_operator_a_jsr_selector_asks_for() {
    let (root, workspace, anchor) = setup();

    pacquet_in(&workspace).with_args(["add", "jsr:@pnpm-e2e/bar@1.0"]).assert().success();

    assert_eq!(
        dependency_spec(&workspace, "dependencies", "@pnpm-e2e/bar").as_deref(),
        Some("jsr:~1.0.1"),
    );
    let lockfile = read_lockfile(&workspace.join(Lockfile::FILE_NAME));
    let entry = root_dependency(&lockfile, "@pnpm-e2e/bar");
    assert_eq!(entry.specifier, "jsr:~1.0.1");
    assert_eq!(entry.version.to_string(), "@jsr/pnpm-e2e__bar@1.0.1");

    drop((root, anchor));
}
