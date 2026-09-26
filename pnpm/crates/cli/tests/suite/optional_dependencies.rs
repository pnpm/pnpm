//! Ports of the TypeScript `optionalDependencies` install suite
//! (`installing/deps-installer/test/install/optionalDependencies.ts`) —
//! see `plans/TEST_PORTING.md` § "Proper Support Of `optionalDependencies`".

pub use _utils::*;

use crate::_utils;

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_lockfile::{Lockfile, PkgName};
use pnpm_testing_utils::bin::CommandTempCwd;
use std::{fs, io, path::Path};

/// Nothing at `path` at all. `Path::exists` follows links, so it also reports
/// `false` for a link whose target is missing — an excluded dependency that was
/// wrongly linked would pass a plain `!exists()` check.
fn is_absent(path: &Path) -> bool {
    match fs::symlink_metadata(path) {
        Ok(_) => false,
        Err(error) if error.kind() == io::ErrorKind::NotFound => true,
        Err(error) => panic!("stat {}: {error}", path.display()),
    }
}

fn read_wanted_lockfile(workspace: &Path) -> Lockfile {
    let text =
        fs::read_to_string(workspace.join(Lockfile::FILE_NAME)).expect("read pnpm-lock.yaml");
    serde_saphyr::from_str(&text).expect("parse pnpm-lock.yaml")
}

fn read_skipped(workspace: &Path) -> Vec<String> {
    pnpm_modules_yaml::read_modules_layout::<pnpm_modules_yaml::Host>(&workspace.join(
        "node_modules",
    ))
    .expect("read .modules.yaml")
    .expect(".modules.yaml exists")
    .skipped
}

fn write_manifest(workspace: &Path, manifest: &serde_json::Value) {
    fs::write(workspace.join("package.json"), manifest.to_string()).expect("write package.json");
}

/// TS: `successfully install optional dependency with subdependencies`
/// (`optionalDependencies.ts:21`). Upstream adds `fsevents`; the port uses
/// the registry-mock fixture with the same shape (an optional dependency
/// that has dependencies of its own).
#[test]
fn install_optional_dependency_with_subdependencies() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();

    pacquet
        .with_args(["add", "--save-optional", "@pnpm.e2e/pkg-with-optional"])
        .assert()
        .success();

    assert!(
        workspace.join("node_modules/@pnpm.e2e/pkg-with-optional/package.json").exists(),
        "the optional dependency must be installed",
    );

    drop((root, npmrc_info)); // cleanup
}

/// TS: `skip failing optional dependencies` (`optionalDependencies.ts:27`).
/// The optional dependency's postinstall fails; the install must still
/// succeed and link the parent.
#[test]
fn skip_failing_optional_dependencies() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    append_workspace_yaml_key(&workspace, "dangerouslyAllowAllBuilds", "true");

    pacquet
        .with_args(["add", "@pnpm.e2e/pkg-with-failing-optional-dependency@1.0.0"])
        .assert()
        .success();

    assert!(
        workspace
            .join("node_modules/@pnpm.e2e/pkg-with-failing-optional-dependency/package.json")
            .exists(),
        "the parent of the failing optional dependency must be installed",
    );

    drop((root, npmrc_info)); // cleanup
}

/// TS: `remove an optional dependency whose build failed`
/// (`optionalDependencies.ts`). Under the isolated linker the direct
/// dependency's link may remain, so `is_dir` rather than [`is_absent`].
#[test]
fn remove_optional_dependency_whose_build_failed() {
    for node_linker in ["isolated", "hoisted"] {
        let CommandTempCwd {
            pacquet,
            root,
            workspace,
            npmrc_info,
            ..
        } = CommandTempCwd::init().add_mocked_registry();
        append_workspace_yaml_key(&workspace, "nodeLinker", node_linker);
        append_workspace_yaml_key(
            &workspace,
            "allowBuilds",
            "{ '@pnpm.e2e/failing-postinstall': true }",
        );

        pacquet
            .with_args(["add", "--save-optional", "@pnpm.e2e/failing-postinstall@1.0.0"])
            .assert()
            .success();

        assert!(
            !workspace.join("node_modules/@pnpm.e2e/failing-postinstall").is_dir(),
            "the optional dependency whose build failed must be removed (nodeLinker={node_linker})",
        );
        let lockfile_text =
            fs::read_to_string(workspace.join(Lockfile::FILE_NAME)).expect("read pnpm-lock.yaml");
        assert!(
            lockfile_text.contains("'@pnpm.e2e/failing-postinstall'"),
            "the lockfile must still record the optional dependency:\n{lockfile_text}",
        );

        drop((root, npmrc_info)); // cleanup
    }
}

/// TS: `rebuild removes an optional dependency whose build failed`
/// (`building/commands/test/build/index.ts`).
#[test]
fn rebuild_removes_optional_dependency_whose_build_failed() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    write_manifest(
        &workspace,
        &serde_json::json!({
            "optionalDependencies": { "@pnpm.e2e/failing-postinstall": "1.0.0" },
        }),
    );
    let installed = workspace.join("node_modules/@pnpm.e2e/failing-postinstall");

    pacquet
        .with_args(["install", "--ignore-scripts"])
        .assert()
        .success();
    assert!(installed.is_dir(), "--ignore-scripts must install the package unbuilt");
    append_workspace_yaml_key(
        &workspace,
        "allowBuilds",
        "{ '@pnpm.e2e/failing-postinstall': true }",
    );

    let CommandTempCwd {
        pacquet: rebuild,
        root: rebuild_root,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    rebuild
        .with_current_dir(&workspace)
        .arg("rebuild")
        .assert()
        .success();

    assert!(!installed.is_dir(), "the optional dependency whose build failed must be removed");

    drop((root, npmrc_info, rebuild_root)); // cleanup
}

/// TS: `skip failing optional peer dependencies` (`optionalDependencies.ts:34`).
/// The auto-installed optional peer's postinstall fails; the install must
/// succeed, and the lockfile must record the peer as an optional dependency
/// of the dependent with an `optional: true` snapshot.
#[test]
fn skip_failing_optional_peer_dependencies() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    append_workspace_yaml_key(&workspace, "dangerouslyAllowAllBuilds", "true");

    pacquet
        .with_args([
            "add",
            "@pnpm.e2e/pkg-with-failing-optional-dependency@1.0.0",
            "@pnpm.e2e/pkg-with-failing-optional-peer@1.0.0",
        ])
        .assert()
        .success();

    let lockfile_text =
        fs::read_to_string(workspace.join(Lockfile::FILE_NAME)).expect("read pnpm-lock.yaml");
    assert!(
        lockfile_text.contains(
            "@pnpm.e2e/pkg-with-failing-optional-peer@1.0.0(@pnpm.e2e/pkg-with-failing-postinstall@1.0.0)"
        ),
        "the failing optional peer must be resolved into the dependent's snapshot key:\n{lockfile_text}",
    );
    let lockfile = read_wanted_lockfile(&workspace);
    let snapshots = lockfile.snapshots.as_ref().expect("lockfile has snapshots");
    let failing_postinstall = snapshots
        .iter()
        .find(|(key, _)| key.to_string() == "@pnpm.e2e/pkg-with-failing-postinstall@1.0.0")
        .expect("failing postinstall package has a snapshot")
        .1;
    assert!(failing_postinstall.optional, "the failing optional peer's snapshot must be optional");

    drop((root, npmrc_info)); // cleanup
}

/// TS: `skip non-existing optional dependency` (`optionalDependencies.ts:45`).
/// A root optional dependency that cannot be resolved must not fail the
/// install; the other dependencies install normally.
#[test]
fn skip_non_existing_optional_dependency() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    write_manifest(
        &workspace,
        &serde_json::json!({
            "dependencies": { "is-positive": "1.0.0" },
            "optionalDependencies": { "@pnpm.e2e/i-do-not-exist": "1000" },
        }),
    );

    pacquet
        .with_arg("install")
        .assert()
        .success();

    assert!(
        workspace.join("node_modules/is-positive/package.json").exists(),
        "the resolvable dependency must be installed",
    );
    let lockfile = read_wanted_lockfile(&workspace);
    let root_importer = lockfile.importers
        .get(Lockfile::ROOT_IMPORTER_KEY)
        .expect("lockfile has the root importer");
    let is_positive_name: PkgName = "is-positive".parse().expect("parse the package name");
    let is_positive = root_importer.dependencies
        .as_ref()
        .expect("root importer has dependencies")
        .get(&is_positive_name)
        .expect("is-positive is recorded");
    assert_eq!(is_positive.specifier, "1.0.0");

    drop((root, npmrc_info)); // cleanup
}

/// Regression test for [pnpm/pnpm#3960](https://github.com/pnpm/pnpm/issues/3960).
#[test]
fn frozen_install_skips_the_optional_dependency_the_lockfile_left_out() {
    frozen_install_with_unresolved_optional_dependency(true);
}

#[test]
fn frozen_install_skips_the_only_dependency_when_it_is_unresolved_and_optional() {
    frozen_install_with_unresolved_optional_dependency(false);
}

fn frozen_install_with_unresolved_optional_dependency(has_required_dependency: bool) {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    write_manifest(
        &workspace,
        &serde_json::json!({
            "dependencies": if has_required_dependency {
                serde_json::json!({ "is-positive": "1.0.0" })
            } else {
                serde_json::json!({})
            },
            "optionalDependencies": { "@pnpm.e2e/i-do-not-exist": "1000" },
        }),
    );
    const SKIP_NOTICE: &str = "info: @pnpm.e2e/i-do-not-exist@1000 is an optional dependency that could not be resolved. Excluding it from installation.";

    let assert = pacquet
        .with_arg("install")
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    assert!(
        stdout.contains(SKIP_NOTICE),
        "the resolving install must report the skip; got:\n{stdout}",
    );
    let lockfile_before =
        fs::read_to_string(workspace.join(Lockfile::FILE_NAME)).expect("read pnpm-lock.yaml");

    for frozen in [["install", "--frozen-lockfile"].as_slice(), ["install"].as_slice()] {
        if workspace.join("node_modules").exists() {
            fs::remove_dir_all(workspace.join("node_modules")).expect("remove node_modules");
        }
        // The test environment pins `PNPM_CONFIG_CI=false`; the bare
        // `install` relies on `CI=true` turning the frozen default on.
        let mut command = pacquet_in(&workspace);
        command
            .env_remove("PNPM_CONFIG_CI")
            .env("CI", "true")
            .args(frozen);
        let assert = command.assert().success();
        let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
        if has_required_dependency {
            assert!(
                stdout.contains("Lockfile is up to date, resolution step is skipped"),
                "{frozen:?} must install from the lockfile; got:\n{stdout}",
            );
        } else {
            assert!(
                stdout.contains("Already up to date"),
                "{frozen:?} must report up to date; got:\n{stdout}",
            );
        }
        assert!(stdout.contains(SKIP_NOTICE), "{frozen:?} must report the skip; got:\n{stdout}");
        if has_required_dependency {
            assert!(
                workspace.join("node_modules/is-positive/package.json").exists(),
                "the resolvable dependency must be installed",
            );
        }
        assert!(
            is_absent(&workspace.join("node_modules/@pnpm.e2e/i-do-not-exist")),
            "the unresolvable optional dependency must be skipped",
        );
        assert_eq!(
            fs::read_to_string(workspace.join(Lockfile::FILE_NAME)).expect("read pnpm-lock.yaml"),
            lockfile_before,
            "a frozen install must not rewrite the lockfile",
        );
    }

    drop((root, npmrc_info)); // cleanup
}

/// TS: `do not skip optional dependency that does not support the current
/// pnpm version` (`optionalDependencies.ts:169`). `engines.pnpm` must not
/// make an optional dependency uninstallable.
#[test]
fn do_not_skip_optional_dependency_that_does_not_support_the_current_pnpm_version() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    write_manifest(
        &workspace,
        &serde_json::json!({
            "optionalDependencies": { "@pnpm.e2e/for-legacy-pnpm": "*" },
        }),
    );

    pacquet
        .with_arg("install")
        .assert()
        .success();

    assert!(
        workspace.join("node_modules/@pnpm.e2e/for-legacy-pnpm/package.json").exists(),
        "an optional dependency constrained on a legacy pnpm must still install",
    );
    assert_eq!(read_skipped(&workspace), Vec::<String>::new());

    drop((root, npmrc_info)); // cleanup
}

/// TS: `only skip optional dependencies`
/// (`optionalDependencies.ts:610`).
#[test]
fn only_optional_dependencies_are_skipped_in_a_mixed_graph() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    write_manifest(
        &workspace,
        &serde_json::json!({
            "dependencies": {
                "@pnpm.e2e/optional-graph-required-root": "1.0.0",
            },
            "optionalDependencies": {
                "@pnpm.e2e/optional-graph-skipped-root": "1.0.0",
            },
        }),
    );

    pacquet
        .with_arg("install")
        .assert()
        .success();

    let virtual_store = workspace.join("node_modules/.pnpm");
    assert!(!virtual_store.join("@pnpm.e2e+optional-graph-skipped-root@1.0.0").exists());
    assert!(virtual_store.join("@pnpm.e2e+optional-graph-shared-middle@1.0.0").exists());
    assert!(virtual_store.join("@pnpm.e2e+optional-graph-shared-leaf@1.0.0").exists());

    drop((root, npmrc_info));
}

/// TS: `complex scenario with same optional dependencies appearing in many
/// places of the dependency graph` (`optionalDependencies.ts:914`).
#[test]
fn repeated_optional_dependencies_across_a_complex_graph_are_classified_per_edge() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    write_arch_workspace_yaml(&workspace, "darwin", "x64", "");
    write_manifest(
        &workspace,
        &serde_json::json!({
            "dependencies": {
                "@pnpm.e2e/optional-selector-parent-a": "1.0.0",
                "@pnpm.e2e/optional-selector-parent-b": "1.0.0",
            },
        }),
    );

    pacquet
        .with_arg("install")
        .assert()
        .success();

    let virtual_store = workspace.join("node_modules/.pnpm");
    for version in ["1.0.0", "2.0.0"] {
        assert!(
            virtual_store
                .join(format!("@pnpm.e2e+optional-platform-selector@{version}"))
                .join("node_modules/@pnpm.e2e/darwin-x64")
                .exists(),
            "the supported optional package must be linked for selector {version}",
        );
    }

    drop((root, npmrc_info));
}

fn sorted_keys<Key: ToString, Value>(map: &std::collections::HashMap<Key, Value>) -> Vec<String> {
    let mut keys: Vec<String> = map
        .keys()
        .map(ToString::to_string)
        .collect();
    keys.sort();
    keys
}

/// TS: `optional subdependency is skipped` (`optionalDependencies.ts:283`),
/// minus the forced-headless tail (covered by
/// [`forced_frozen_install_materializes_incompatible_optionals`]).
#[test]
fn optional_subdependency_is_skipped() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();

    pacquet
        .with_args(["add", "@pnpm.e2e/pkg-with-optional", "@pnpm.e2e/dep-of-optional-pkg"])
        .assert()
        .success();

    assert_eq!(read_skipped(&workspace), ["@pnpm.e2e/not-compatible-with-any-os@1.0.0"]);
    assert!(workspace.join("node_modules/.pnpm/@pnpm.e2e+pkg-with-optional@1.0.0").exists());
    assert!(
        !workspace.join("node_modules/.pnpm/@pnpm.e2e+not-compatible-with-any-os@1.0.0").exists(),
        "the platform-incompatible optional subdependency must not be materialized",
    );

    // Recreate the lockfile: the skipped optional must be resolved back in.
    fs::remove_file(workspace.join(Lockfile::FILE_NAME)).expect("remove pnpm-lock.yaml");
    pacquet_in(&workspace)
        .with_arg("install")
        .assert()
        .success();

    let lockfile = read_wanted_lockfile(&workspace);
    let packages = lockfile.packages.as_ref().expect("lockfile has packages");
    assert_eq!(packages.len(), 3, "packages: {:?}", sorted_keys(packages));
    assert!(
        packages
            .keys()
            .any(|key| key.to_string() == "@pnpm.e2e/not-compatible-with-any-os@1.0.0"),
        "the recreated lockfile must resolve the skipped optional",
    );

    drop((root, npmrc_info)); // cleanup
}

/// TS: `only that package is skipped which is an optional dependency only
/// and not installable` (`optionalDependencies.ts:359`). A
/// platform-incompatible package that is *also* a regular dependency is
/// installed, and nothing lands in the skip set.
#[test]
fn only_optional_only_and_not_installable_package_is_skipped() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();

    pacquet
        .with_args([
            "add",
            "@pnpm.e2e/peer-c@1.0.0",
            "@pnpm.e2e/has-optional-dep-with-peer",
            "@pnpm.e2e/not-compatible-with-any-os-and-has-peer",
        ])
        .assert()
        .success();

    assert_eq!(read_skipped(&workspace), Vec::<String>::new());

    let lockfile = read_wanted_lockfile(&workspace);
    let snapshots = lockfile.snapshots.as_ref().expect("lockfile has snapshots");
    let dep_of_optional = snapshots
        .iter()
        .find(|(key, _)| key.to_string() == "@pnpm.e2e/dep-of-optional-pkg@1.0.0")
        .expect("dep-of-optional-pkg has a snapshot")
        .1;
    assert!(
        !dep_of_optional.optional,
        "a package reachable through a regular dependency must not be optional-only",
    );

    fs::remove_dir_all(workspace.join("node_modules")).expect("remove node_modules");
    pacquet_in(&workspace)
        .with_args(["install", "--frozen-lockfile"])
        .assert()
        .success();

    assert_eq!(read_skipped(&workspace), Vec::<String>::new());

    drop((root, npmrc_info)); // cleanup
}

/// TS: `not installing optional deps` (`deps-restorer/test/index.ts:323`):
/// a headless install with the optional group excluded skips the optional
/// dependency entirely — as the root's own optional and as the transitive
/// optional of `pkg-with-good-optional` — while regular dependencies
/// install.
#[test]
fn headless_install_without_optional_deps() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    write_manifest(
        &workspace,
        &serde_json::json!({
            "dependencies": { "@pnpm.e2e/pkg-with-good-optional": "1.0.0" },
            "optionalDependencies": { "is-positive": "1.0.0" },
        }),
    );

    pacquet
        .with_args(["install", "--lockfile-only"])
        .assert()
        .success();
    pacquet_in(&workspace)
        .with_args(["install", "--frozen-lockfile", "--no-optional"])
        .assert()
        .success();

    assert!(
        workspace.join("node_modules/@pnpm.e2e/pkg-with-good-optional/package.json").exists(),
        "the regular dependency must be installed",
    );
    assert!(
        !workspace.join("node_modules/is-positive").exists(),
        "the root optional dependency must not be linked",
    );
    assert!(
        !workspace.join("node_modules/.pnpm/is-positive@1.0.0").exists(),
        "the optional dependency must not be materialized at all",
    );

    drop((root, npmrc_info)); // cleanup
}

/// TS: `installing only optional deps` (`deps-restorer/test/index.ts:300`).
/// Upstream drives the programmatic API with `include = { dependencies:
/// false, devDependencies: false, optionalDependencies: true }`, which no
/// flag combination reaches. `--dev` is the closest CLI-reachable headless
/// include filter: it drops the optional group along with the production one.
#[test]
fn headless_install_include_filtering_excludes_production_group() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    write_manifest(
        &workspace,
        &serde_json::json!({
            "dependencies": { "@pnpm.e2e/foo": "^100.0.0" },
            "devDependencies": { "@pnpm.e2e/bar": "^100.0.0" },
            "optionalDependencies": { "@pnpm.e2e/qar": "^100.0.0" },
        }),
    );

    pacquet
        .with_args(["install", "--lockfile-only"])
        .assert()
        .success();
    pacquet_in(&workspace)
        .with_args(["install", "--frozen-lockfile", "--dev"])
        .assert()
        .success();

    assert!(
        is_absent(&workspace.join("node_modules/@pnpm.e2e/foo")),
        "the excluded production dependency must not be linked",
    );
    assert!(
        workspace.join("node_modules/@pnpm.e2e/bar/package.json").exists(),
        "the dev dependency must be installed",
    );
    assert!(
        is_absent(&workspace.join("node_modules/@pnpm.e2e/qar")),
        "a dev-only install must not install the optional dependency",
    );

    drop((root, npmrc_info)); // cleanup
}

/// TS: `skipping optional dependency if it cannot be fetched`
/// (`deps-restorer/test/index.ts:340`): a headless install must swallow a
/// failed optional fetch, install the rest, and still write the install
/// state files. Upstream's fixture points the optional at an unresolvable
/// tarball; the port breaks the optional's stored integrity so the fetch
/// fails verification.
#[test]
fn headless_install_skips_unfetchable_optional_dependency() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    // Upstream runs with `retry: { retries: 0 }`; without it the failed
    // fetch retries with backoff and dominates the test's runtime.
    append_workspace_yaml_key(&workspace, "fetchRetries", "0");
    write_manifest(
        &workspace,
        &serde_json::json!({
            "dependencies": { "is-positive": "1.0.0" },
            "optionalDependencies": { "@pnpm.e2e/foo": "100.0.0" },
        }),
    );

    pacquet
        .with_args(["install", "--lockfile-only"])
        .assert()
        .success();

    // Corrupt the optional's integrity: the tarball then fails
    // verification on fetch. The store and cache must be cold, or the
    // already-verified copy would satisfy the install without a fetch.
    let lockfile_path = workspace.join(Lockfile::FILE_NAME);
    let lockfile_text = fs::read_to_string(&lockfile_path).expect("read pnpm-lock.yaml");
    let foo_header = "'@pnpm.e2e/foo@100.0.0':";
    let foo_at = lockfile_text.find(foo_header).expect("lockfile records the optional");
    let integrity_at =
        foo_at + lockfile_text[foo_at..].find("sha512-").expect("optional has an integrity");
    let integrity_end =
        integrity_at + lockfile_text[integrity_at..].find('}').expect("integrity value ends");
    let mut corrupted = lockfile_text.clone();
    corrupted.replace_range(integrity_at..integrity_end, &format!("sha512-{}==", "A".repeat(86)));
    assert_ne!(corrupted, lockfile_text);
    fs::write(&lockfile_path, corrupted).expect("write the corrupted lockfile");
    fs::remove_dir_all(&npmrc_info.store_dir).expect("clear the store");
    fs::remove_dir_all(&npmrc_info.cache_dir).expect("clear the cache");

    pacquet_in(&workspace)
        .with_args(["install", "--frozen-lockfile"])
        .assert()
        .success();

    assert!(
        workspace.join("node_modules/is-positive/package.json").exists(),
        "the regular dependency must be installed",
    );
    assert!(
        !workspace.join("node_modules/@pnpm.e2e/foo").exists(),
        "the unfetchable optional dependency must be skipped",
    );
    assert!(
        workspace.join("node_modules/.pnpm/lock.yaml").exists(),
        "the current lockfile must still be written",
    );
    assert!(
        workspace.join("node_modules/.modules.yaml").exists(),
        ".modules.yaml must still be written",
    );

    drop((root, npmrc_info)); // cleanup
}

/// TS: `optional dependency is hardlinked to the store if it does not
/// require a build` (`optionalDependencies.ts:665`). Upstream asserts the
/// `pnpm:progress` `method: hardlink` emission; the port asserts the
/// observable outcome — the materialized file shares an inode with the
/// store — on both the fresh and the frozen path.
#[cfg(unix)]
#[test]
fn optional_dependency_is_hardlinked_to_the_store_if_it_does_not_require_a_build() {
    use std::os::unix::fs::MetadataExt;

    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    append_workspace_yaml_key(&workspace, "packageImportMethod", "hardlink");
    write_manifest(
        &workspace,
        &serde_json::json!({
            "dependencies": { "@pnpm.e2e/pkg-with-good-optional": "*" },
        }),
    );

    let assert_hardlinked = || {
        let file = workspace.join(
            "node_modules/.pnpm/is-positive@1.0.0/node_modules/is-positive/package.json",
        );
        let metadata = fs::metadata(&file).expect("materialized optional dependency file exists");
        assert!(
            metadata.nlink() > 1,
            "the optional dependency must be hardlinked to the store (nlink = {})",
            metadata.nlink(),
        );
    };

    pacquet
        .with_arg("install")
        .assert()
        .success();
    assert_hardlinked();

    fs::remove_dir_all(workspace.join("node_modules")).expect("remove node_modules");
    pacquet_in(&workspace)
        .with_args(["install", "--frozen-lockfile"])
        .assert()
        .success();
    assert_hardlinked();

    drop((root, npmrc_info)); // cleanup
}

/// TS: `optional subdependency of newly added optional dependency is
/// skipped` (`optionalDependencies.ts:344`, pnpm/pnpm issue 2663).
#[test]
fn optional_subdependency_of_newly_added_optional_dependency_is_skipped() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();

    pacquet
        .with_args(["add", "--save-optional", "@pnpm.e2e/pkg-with-optional"])
        .assert()
        .success();

    assert_eq!(
        read_skipped(&workspace),
        ["@pnpm.e2e/dep-of-optional-pkg@1.0.0", "@pnpm.e2e/not-compatible-with-any-os@1.0.0"],
    );
    let lockfile = read_wanted_lockfile(&workspace);
    let packages = lockfile.packages.as_ref().expect("lockfile has packages");
    assert_eq!(packages.len(), 3, "packages: {:?}", sorted_keys(packages));

    drop((root, npmrc_info)); // cleanup
}

/// TS: `optional dependency has bigger priority than regular dependency`
/// (`optionalDependencies.ts:419`): the same name in `dependencies` and
/// `optionalDependencies` resolves to the optional entry's version.
#[test]
fn optional_dependency_has_bigger_priority_than_regular_dependency() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    write_manifest(
        &workspace,
        &serde_json::json!({
            "dependencies": { "is-positive": "1.0.0" },
            "optionalDependencies": { "is-positive": "3.1.0" },
        }),
    );

    pacquet
        .with_arg("install")
        .assert()
        .success();

    let manifest: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(workspace.join("node_modules/is-positive/package.json"))
            .expect("read the installed manifest"),
    )
    .expect("parse the installed manifest");
    assert_eq!(manifest["version"], "3.1.0");

    drop((root, npmrc_info)); // cleanup
}

/// TS: `do not fail on an optional dependency that has a non-optional
/// dependency with a failing postinstall script`
/// (`optionalDependencies.ts:563`).
#[test]
fn do_not_fail_on_optional_dependency_with_failing_non_optional_postinstall() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    append_workspace_yaml_key(&workspace, "dangerouslyAllowAllBuilds", "true");

    pacquet
        .with_args(["add", "--save-optional", "@pnpm.e2e/has-failing-postinstall-dep@1.0.0"])
        .assert()
        .success();

    drop((root, npmrc_info)); // cleanup
}

/// TS: `fail on a package with failing postinstall if the package is both
/// an optional and non-optional dependency` (`optionalDependencies.ts:574`).
#[test]
fn fail_on_failing_postinstall_when_package_is_both_optional_and_non_optional() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    append_workspace_yaml_key(&workspace, "dangerouslyAllowAllBuilds", "true");
    write_manifest(
        &workspace,
        &serde_json::json!({
            "dependencies": { "@pnpm.e2e/failing-postinstall": "1.0.0" },
            "optionalDependencies": { "@pnpm.e2e/has-failing-postinstall-dep": "1.0.0" },
        }),
    );

    pacquet
        .with_arg("install")
        .assert()
        .failure();

    drop((root, npmrc_info)); // cleanup
}

/// Rewrite `pnpm-workspace.yaml` with the harness anchors plus
/// `supportedArchitectures` and `extra` — upstream drives the
/// architecture change through configuration, which also invalidates the
/// repeat-install fast path the way a config edit does for pnpm.
fn write_arch_workspace_yaml(workspace: &Path, os: &str, cpu: &str, extra: &str) {
    let yaml = format!(
        "storeDir: ../pacquet-store\ncacheDir: ../pacquet-cache\nenableGlobalVirtualStore: false\n{extra}supportedArchitectures:\n  os: [{os}]\n  cpu: [{cpu}]\n",
    );
    fs::write(workspace.join("pnpm-workspace.yaml"), yaml).expect("write pnpm-workspace.yaml");
}

/// TS: `remove optional dependencies that are not used`
/// (`optionalDependencies.ts:618`). Narrowing `supportedArchitectures`
/// on a later install prunes the platform packages the new set no longer
/// needs (`modulesCacheMaxAge: 0` makes the sweep run every install).
#[test]
fn remove_optional_dependencies_that_are_not_used() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    write_arch_workspace_yaml(
        &workspace,
        "darwin, linux, win32",
        "arm64, x64",
        "modulesCacheMaxAge: 0\n",
    );

    pacquet
        .with_args(["add", "@pnpm.e2e/has-many-optional-deps@1.0.0"])
        .assert()
        .success();
    let virtual_store = workspace.join("node_modules/.pnpm");
    for name in ["darwin-arm64", "darwin-x64", "linux-x64", "windows-x64"] {
        assert!(
            virtual_store
                .join(format!("@pnpm.e2e+{name}@1.0.0"))
                .exists(),
            "{name} must be materialized under the broad architecture set",
        );
    }

    write_arch_workspace_yaml(&workspace, "darwin", "x64", "modulesCacheMaxAge: 0\n");
    pacquet_in(&workspace)
        .with_arg("install")
        .assert()
        .success();
    assert!(virtual_store.join("@pnpm.e2e+darwin-x64@1.0.0").exists());
    for name in ["darwin-arm64", "linux-x64", "windows-x64"] {
        assert!(
            !virtual_store
                .join(format!("@pnpm.e2e+{name}@1.0.0"))
                .exists(),
            "{name} must be pruned once the architecture set no longer needs it",
        );
    }

    drop((root, npmrc_info)); // cleanup
}

/// TS: `remove optional dependencies that are not used, when hoisted node
/// linker is used` (`optionalDependencies.ts:633`).
#[test]
fn remove_optional_dependencies_that_are_not_used_with_hoisted_linker() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    write_arch_workspace_yaml(
        &workspace,
        "darwin, linux, win32",
        "arm64, x64",
        "nodeLinker: hoisted\n",
    );

    pacquet
        .with_args(["add", "@pnpm.e2e/has-many-optional-deps@1.0.0"])
        .assert()
        .success();

    write_arch_workspace_yaml(&workspace, "darwin", "x64", "nodeLinker: hoisted\n");
    pacquet_in(&workspace)
        .with_arg("install")
        .assert()
        .success();

    let mut entries: Vec<String> = fs::read_dir(workspace.join("node_modules/@pnpm.e2e"))
        .expect("read node_modules/@pnpm.e2e")
        .map(|entry| {
            entry
                .expect("read dir entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    entries.sort();
    assert_eq!(entries, ["darwin-x64", "has-many-optional-deps"]);

    drop((root, npmrc_info)); // cleanup
}

/// TS: `optional subdependency is not removed from current lockfile when
/// new dependency added` (`optionalDependencies.ts:213`, pnpm/pnpm issue
/// 2636): in a workspace with a shared lockfile, an `add` in one project
/// must keep the other project's skipped-optional metadata in the current
/// lockfile.
#[test]
fn optional_subdependency_stays_in_current_lockfile_when_new_dependency_added() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    append_workspace_yaml_key(&workspace, "packages", "['project-1', 'project-2']");
    write_manifest(&workspace, &serde_json::json!({ "name": "root", "private": true }));
    for (name, manifest) in [
        (
            "project-1",
            serde_json::json!({
                "name": "project-1",
                "version": "1.0.0",
                "dependencies": { "@pnpm.e2e/pkg-with-optional": "1.0.0" },
            }),
        ),
        ("project-2", serde_json::json!({ "name": "project-2", "version": "1.0.0" })),
    ] {
        fs::create_dir_all(workspace.join(name)).expect("create project dir");
        fs::write(workspace.join(name).join("package.json"), manifest.to_string())
            .expect("write project manifest");
    }

    pacquet
        .with_arg("install")
        .assert()
        .success();

    assert_eq!(
        read_skipped(&workspace),
        ["@pnpm.e2e/dep-of-optional-pkg@1.0.0", "@pnpm.e2e/not-compatible-with-any-os@1.0.0"],
    );
    let current_has_not_compatible = |current: &Lockfile| {
        current.packages
            .as_ref()
            .is_some_and(|packages| {
                packages
                    .keys()
                    .any(|key| key.to_string() == "@pnpm.e2e/not-compatible-with-any-os@1.0.0")
            })
    };
    assert!(
        current_has_not_compatible(&read_current_lockfile(&workspace)),
        "the skipped optional's metadata must be in the current lockfile",
    );

    pacquet_in(&workspace.join("project-1"))
        .with_args(["add", "is-positive@1.0.0"])
        .assert()
        .success();

    assert!(
        current_has_not_compatible(&read_current_lockfile(&workspace)),
        "an add in a sibling project must not drop the skipped optional's metadata",
    );

    drop((root, npmrc_info)); // cleanup
}

mod platforms;

mod filtering;
