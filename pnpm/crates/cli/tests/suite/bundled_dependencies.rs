//! Ports of the TypeScript `bundledDependencies` install suite
//! (`installing/deps-installer/test/install/bundledDependencies.ts`).

pub use _utils::{append_workspace_yaml_key, assert_bin_linked, pacquet_in};

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_lockfile::{BundledDependencies, Lockfile, PackageMetadata};
use pnpm_testing_utils::{
    bin::CommandTempCwd, fixtures::tarball_entries, fs::is_symlink_or_junction,
};
use std::{fs, path::Path};

use crate::_utils;

#[test]
fn bundled_dependencies_are_kept_out_of_the_lockfile() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();

    pacquet
        .with_args(["add", "@pnpm.e2e/pkg-with-bundled-dependencies@1.0.0"])
        .assert()
        .success();

    assert_bin_linked(&workspace.join(
        "node_modules/@pnpm.e2e/pkg-with-bundled-dependencies/node_modules/.bin/hello-world-js-bin",
    ));

    let lockfile = read_wanted_lockfile(&workspace);
    assert_eq!(
        package(&lockfile, "@pnpm.e2e/pkg-with-bundled-dependencies@1.0.0").bundled_dependencies,
        Some(BundledDependencies::Names(vec!["@pnpm.e2e/hello-world-js-bin".to_string()])),
    );
    assert!(
        !has_package(&lockfile, "@pnpm.e2e/hello-world-js-bin@1.0.0"),
        "a bundled dependency ships inside the tarball and must not get a lockfile entry",
    );

    drop((root, npmrc_info)); // cleanup
}

#[test]
fn bundle_dependencies_spelling_is_kept_out_of_the_lockfile() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();

    pacquet
        .with_args(["add", "@pnpm.e2e/pkg-with-bundle-dependencies@1.0.0"])
        .assert()
        .success();

    assert_bin_linked(&workspace.join(
        "node_modules/@pnpm.e2e/pkg-with-bundle-dependencies/node_modules/.bin/hello-world-js-bin",
    ));

    let lockfile = read_wanted_lockfile(&workspace);
    assert_eq!(
        package(&lockfile, "@pnpm.e2e/pkg-with-bundle-dependencies@1.0.0").bundled_dependencies,
        Some(BundledDependencies::Names(vec!["@pnpm.e2e/hello-world-js-bin".to_string()])),
        "the lockfile records the `bundleDependencies` spelling under `bundledDependencies`",
    );
    assert!(!has_package(&lockfile, "@pnpm.e2e/hello-world-js-bin@1.0.0"));

    drop((root, npmrc_info)); // cleanup
}

#[test]
fn bundle_dependencies_true_is_recorded_as_true() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();

    pacquet
        .with_args(["add", "@pnpm.e2e/pkg-with-bundle-dependencies-true@1.0.0"])
        .assert()
        .success();

    let lockfile = read_wanted_lockfile(&workspace);
    assert_eq!(
        package(&lockfile, "@pnpm.e2e/pkg-with-bundle-dependencies-true@1.0.0")
            .bundled_dependencies,
        Some(BundledDependencies::Boolean(true)),
        "`bundleDependencies: true` is recorded verbatim, and drives bundled-bin linking",
    );
    assert!(!has_package(&lockfile, "@pnpm.e2e/hello-world-js-bin@1.0.0"));

    let bundled_bin = workspace.join(
        "node_modules/@pnpm.e2e/pkg-with-bundle-dependencies-true/node_modules/.bin/hello-world-js-bin",
    );
    assert_bin_linked(&bundled_bin);

    // The boolean form has to survive a round trip through the lockfile,
    // both to parse at all and to keep driving the bundled-bin linking.
    fs::remove_dir_all(workspace.join("node_modules")).expect("remove node_modules");
    pacquet_in(&workspace)
        .with_args(["install", "--frozen-lockfile"])
        .assert()
        .success();
    assert_bin_linked(&bundled_bin);

    drop((root, npmrc_info)); // cleanup
}

#[test]
fn bundled_bins_are_linked_under_the_hoisted_linker() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();

    append_workspace_yaml_key(&workspace, "nodeLinker", "hoisted");

    // Both declaration shapes, because the hoisted linker reaches the bundling
    // signal through `DependenciesGraphNode::has_bundled_dependencies` rather
    // than the lockfile row the isolated linker reads.
    pacquet
        .with_args([
            "add",
            "@pnpm.e2e/pkg-with-bundled-dependencies@1.0.0",
            "@pnpm.e2e/pkg-with-bundle-dependencies-true@1.0.0",
        ])
        .assert()
        .success();

    for bundling_pkg in
        ["@pnpm.e2e/pkg-with-bundled-dependencies", "@pnpm.e2e/pkg-with-bundle-dependencies-true"]
    {
        let pkg_dir = workspace.join("node_modules").join(bundling_pkg);
        // The hoisted linker materializes a real directory where the isolated
        // one leaves a symlink into the virtual store, so this is what proves
        // the `nodeLinker` key took effect and the other linker is not what
        // linked the bin below.
        assert!(
            pkg_dir.is_dir() && !is_symlink_or_junction(&pkg_dir).expect("stat the package dir"),
            "{bundling_pkg} must be a real directory under the hoisted linker",
        );
        assert_bin_linked(&pkg_dir.join("node_modules/.bin/hello-world-js-bin"));
    }

    drop((root, npmrc_info)); // cleanup
}

#[test]
fn bundle_dependencies_false_is_not_recorded() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();

    pacquet
        .with_args(["add", "@pnpm.e2e/pkg-with-bundle-dependencies-false"])
        .assert()
        .success();

    let lockfile = read_wanted_lockfile(&workspace);
    assert_eq!(
        package(&lockfile, "@pnpm.e2e/pkg-with-bundle-dependencies-false@1.0.0")
            .bundled_dependencies,
        None,
    );
    assert!(
        has_package(&lockfile, "@pnpm.e2e/hello-world-js-bin@1.0.0"),
        "`bundleDependencies: false` bundles nothing, so the dependency is resolved normally",
    );

    drop((root, npmrc_info)); // cleanup
}

#[test]
fn bundled_dependencies_survive_a_lockfile_rewrite() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();

    pacquet
        .with_args(["add", "@pnpm.e2e/pkg-with-bundled-dependencies@1.0.0"])
        .assert()
        .success();

    // Adding an unrelated package rewrites the lockfile while the first
    // entry's resolution is reused rather than resolved again.
    pacquet_in(&workspace)
        .with_args(["add", "@pnpm.e2e/foo@100.0.0"])
        .assert()
        .success();

    let lockfile = read_wanted_lockfile(&workspace);
    assert_eq!(
        package(&lockfile, "@pnpm.e2e/pkg-with-bundled-dependencies@1.0.0").bundled_dependencies,
        Some(BundledDependencies::Names(vec!["@pnpm.e2e/hello-world-js-bin".to_string()])),
        "an entry whose resolution was reused keeps its recorded bundled dependencies",
    );

    drop((root, npmrc_info)); // cleanup
}

/// Covers <https://github.com/pnpm/pnpm/issues/7411>.
#[test]
fn local_tarball_with_bundled_dependencies() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();

    write_tarball_bundling_hello_world_js_bin(
        &workspace.join("pkg.tgz"),
        &serde_json::json!({
            "name": "@pnpm.e2e/pkg-with-bundled-dependencies",
            "version": "1.0.0",
            "bundledDependencies": ["@pnpm.e2e/hello-world-js-bin"],
            "dependencies": { "@pnpm.e2e/hello-world-js-bin": "1.0.0" },
        }),
    );

    pacquet
        .with_args(["add", "file:pkg.tgz"])
        .assert()
        .success();

    let lockfile = read_wanted_lockfile(&workspace);
    assert_eq!(
        package(&lockfile, "@pnpm.e2e/pkg-with-bundled-dependencies@file:pkg.tgz")
            .bundled_dependencies,
        Some(BundledDependencies::Names(vec!["@pnpm.e2e/hello-world-js-bin".to_string()])),
    );
    assert!(!has_any_version_of(&lockfile, "@pnpm.e2e/hello-world-js-bin"));

    drop((root, npmrc_info)); // cleanup
}

#[test]
fn local_tarball_with_bundle_dependencies_true() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();

    write_tarball_bundling_hello_world_js_bin(
        &workspace.join("pkg.tgz"),
        &serde_json::json!({
            "name": "@pnpm.e2e/pkg-with-bundle-dependencies-true",
            "version": "1.0.0",
            "bundleDependencies": true,
            "dependencies": { "@pnpm.e2e/hello-world-js-bin": "*" },
        }),
    );

    pacquet
        .with_args(["add", "file:pkg.tgz"])
        .assert()
        .success();

    let lockfile = read_wanted_lockfile(&workspace);
    assert_eq!(
        package(&lockfile, "@pnpm.e2e/pkg-with-bundle-dependencies-true@file:pkg.tgz")
            .bundled_dependencies,
        Some(BundledDependencies::Boolean(true)),
    );
    assert!(!has_any_version_of(&lockfile, "@pnpm.e2e/hello-world-js-bin"));

    drop((root, npmrc_info)); // cleanup
}

/// Writes a package tarball laid out like the registry fixtures' packed
/// output: `manifest` plus `@pnpm.e2e/hello-world-js-bin` under its
/// `node_modules`.
fn write_tarball_bundling_hello_world_js_bin(path: &Path, manifest: &serde_json::Value) {
    let manifest = manifest.to_string();
    let bundled_manifest = serde_json::json!({
        "name": "@pnpm.e2e/hello-world-js-bin",
        "version": "1.0.0",
        "bin": "./index.js",
    })
    .to_string();
    let tarball = tarball_entries(&[
        ("package/package.json", manifest.as_bytes()),
        (
            "package/node_modules/@pnpm.e2e/hello-world-js-bin/package.json",
            bundled_manifest.as_bytes(),
        ),
        (
            "package/node_modules/@pnpm.e2e/hello-world-js-bin/index.js",
            b"#!/usr/bin/env node\nconsole.log('Hello world!')\n",
        ),
    ]);
    fs::write(path, tarball).expect("write tarball");
}

fn read_wanted_lockfile(workspace: &Path) -> Lockfile {
    let text =
        fs::read_to_string(workspace.join(Lockfile::FILE_NAME)).expect("read pnpm-lock.yaml");
    serde_saphyr::from_str(&text).expect("parse pnpm-lock.yaml")
}

fn package<'a>(lockfile: &'a Lockfile, key: &str) -> &'a PackageMetadata {
    lockfile.packages
        .as_ref()
        .expect("lockfile has packages")
        .iter()
        .find_map(|(candidate, metadata)| (candidate.to_string() == key).then_some(metadata))
        .unwrap_or_else(|| panic!("the lockfile must record {key}"))
}

fn has_package(lockfile: &Lockfile, key: &str) -> bool {
    lockfile.packages
        .as_ref()
        .is_some_and(|packages| {
            packages
                .keys()
                .any(|candidate| candidate.to_string() == key)
        })
}

fn has_any_version_of(lockfile: &Lockfile, name: &str) -> bool {
    lockfile.packages
        .as_ref()
        .is_some_and(|packages| {
            packages
                .keys()
                .any(|candidate| candidate.name.to_string() == name)
        })
}
