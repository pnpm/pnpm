//! Ports of the TypeScript `bundledDependencies` install suite
//! (`installing/deps-installer/test/install/bundledDependencies.ts`).

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pacquet_lockfile::{BundledDependencies, Lockfile, PackageMetadata};
use pacquet_testing_utils::{bin::CommandTempCwd, fs::is_symlink_or_junction};
use std::{fs, path::Path};

pub mod _utils;
pub use _utils::{append_workspace_yaml_key, pacquet_in};

#[test]
fn bundled_dependencies_are_kept_out_of_the_lockfile() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();

    pacquet.with_args(["add", "@pnpm.e2e/pkg-with-bundled-dependencies@1.0.0"]).assert().success();

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
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();

    pacquet.with_args(["add", "@pnpm.e2e/pkg-with-bundle-dependencies@1.0.0"]).assert().success();

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
    assert_bin_linked(&bundled_bin);

    // The boolean form has to survive a round trip through the lockfile,
    // both to parse at all and to keep driving the bundled-bin linking.
    fs::remove_dir_all(workspace.join("node_modules")).expect("remove node_modules");
    pacquet_in(&workspace).with_args(["install", "--frozen-lockfile"]).assert().success();
    assert_bin_linked(&bundled_bin);

    drop((root, npmrc_info)); // cleanup
}

#[test]
fn bundled_bins_are_linked_under_the_hoisted_linker() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();

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

/// A linked bin means something different per platform: Unix has the
/// executable bit on the extensionless shim, while Windows has no such bit and
/// instead relies on the `.cmd` / `.ps1` launchers written next to it. Assert
/// whichever of the two actually makes the bin invocable on the host.
fn assert_bin_linked(shim: &Path) {
    assert!(shim.exists(), "the bundled dependency's bin must be linked at {shim:?}");
    #[cfg(unix)]
    assert!(
        pacquet_testing_utils::fs::is_path_executable(shim),
        "the bundled dependency's bin shim at {shim:?} must be executable",
    );
    #[cfg(windows)]
    for extension in ["cmd", "ps1"] {
        let launcher = shim.with_file_name(format!(
            "{}.{extension}",
            shim.file_name().expect("bin shim has a file name").to_string_lossy(),
        ));
        assert!(launcher.exists(), "the bin shim at {shim:?} needs its {extension} launcher");
    }
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
