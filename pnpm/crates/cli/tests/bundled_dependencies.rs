//! Ports of the TypeScript `bundledDependencies` install suite
//! (`installing/deps-installer/test/install/bundledDependencies.ts`).

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pacquet_lockfile::{BundledDependencies, Lockfile, PackageMetadata};
use pacquet_testing_utils::{bin::CommandTempCwd, fs::is_path_executable};
use std::{fs, path::Path};

pub mod _utils;
pub use _utils::pacquet_in;

#[test]
fn bundled_dependencies_are_kept_out_of_the_lockfile() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();

    pacquet.with_args(["add", "@pnpm.e2e/pkg-with-bundled-dependencies@1.0.0"]).assert().success();

    assert!(
        is_path_executable(&workspace.join(
            "node_modules/@pnpm.e2e/pkg-with-bundled-dependencies/node_modules/.bin/hello-world-js-bin"
        )),
        "the bundled dependency's bin must be linked inside the bundling package",
    );

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
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();

    pacquet.with_args(["add", "@pnpm.e2e/pkg-with-bundle-dependencies@1.0.0"]).assert().success();

    assert!(
        is_path_executable(&workspace.join(
            "node_modules/@pnpm.e2e/pkg-with-bundle-dependencies/node_modules/.bin/hello-world-js-bin"
        )),
        "the bundled dependency's bin must be linked inside the bundling package",
    );

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
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();

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
    assert!(is_path_executable(&bundled_bin));

    // The boolean form has to survive a round trip through the lockfile,
    // both to parse at all and to keep driving the bundled-bin linking.
    fs::remove_dir_all(workspace.join("node_modules")).expect("remove node_modules");
    pacquet_in(&workspace).with_args(["install", "--frozen-lockfile"]).assert().success();
    assert!(is_path_executable(&bundled_bin));

    drop((root, npmrc_info)); // cleanup
}

#[test]
fn bundle_dependencies_false_is_not_recorded() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();

    pacquet.with_args(["add", "@pnpm.e2e/pkg-with-bundle-dependencies-false"]).assert().success();

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

fn read_wanted_lockfile(workspace: &Path) -> Lockfile {
    let text =
        fs::read_to_string(workspace.join(Lockfile::FILE_NAME)).expect("read pnpm-lock.yaml");
    serde_saphyr::from_str(&text).expect("parse pnpm-lock.yaml")
}

fn package<'a>(lockfile: &'a Lockfile, key: &str) -> &'a PackageMetadata {
    lockfile
        .packages
        .as_ref()
        .expect("lockfile has packages")
        .iter()
        .find_map(|(candidate, metadata)| (candidate.to_string() == key).then_some(metadata))
        .unwrap_or_else(|| panic!("the lockfile must record {key}"))
}

fn has_package(lockfile: &Lockfile, key: &str) -> bool {
    lockfile
        .packages
        .as_ref()
        .is_some_and(|packages| packages.keys().any(|candidate| candidate.to_string() == key))
}
