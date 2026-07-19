//! Ports of the TypeScript `optionalDependencies` install suite
//! (`installing/deps-installer/test/install/optionalDependencies.ts`) —
//! see `plans/TEST_PORTING.md` § "Proper Support Of `optionalDependencies`".

pub mod _utils;
pub use _utils::*;

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pacquet_lockfile::{Lockfile, PkgName};
use pacquet_testing_utils::bin::CommandTempCwd;
use std::{fs, path::Path, process::Command};

fn pacquet_in(workspace: &Path) -> Command {
    Command::cargo_bin("pnpm").expect("find the pnpm binary").with_current_dir(workspace)
}

fn read_wanted_lockfile(workspace: &Path) -> Lockfile {
    let text =
        fs::read_to_string(workspace.join(Lockfile::FILE_NAME)).expect("read pnpm-lock.yaml");
    serde_saphyr::from_str(&text).expect("parse pnpm-lock.yaml")
}

fn read_skipped(workspace: &Path) -> Vec<String> {
    pacquet_modules_yaml::read_modules_layout::<pacquet_modules_yaml::Host>(
        &workspace.join("node_modules"),
    )
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
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();

    pacquet.with_args(["add", "--save-optional", "@pnpm.e2e/pkg-with-optional"]).assert().success();

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
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
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

/// TS: `skip failing optional peer dependencies` (`optionalDependencies.ts:34`).
/// The auto-installed optional peer's postinstall fails; the install must
/// succeed, and the lockfile must record the peer as an optional dependency
/// of the dependent with an `optional: true` snapshot.
#[test]
fn skip_failing_optional_peer_dependencies() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
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
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    write_manifest(
        &workspace,
        &serde_json::json!({
            "dependencies": { "is-positive": "1.0.0" },
            "optionalDependencies": { "@pnpm.e2e/i-do-not-exist": "1000" },
        }),
    );

    pacquet.with_arg("install").assert().success();

    assert!(
        workspace.join("node_modules/is-positive/package.json").exists(),
        "the resolvable dependency must be installed",
    );
    let lockfile = read_wanted_lockfile(&workspace);
    let root_importer = lockfile
        .importers
        .get(Lockfile::ROOT_IMPORTER_KEY)
        .expect("lockfile has the root importer");
    let is_positive_name: PkgName = "is-positive".parse().expect("parse the package name");
    let is_positive = root_importer
        .dependencies
        .as_ref()
        .expect("root importer has dependencies")
        .get(&is_positive_name)
        .expect("is-positive is recorded");
    assert_eq!(is_positive.specifier, "1.0.0");

    drop((root, npmrc_info)); // cleanup
}

/// TS: `skip optional dependency that does not support the current Node
/// version` (`optionalDependencies.ts:143`).
#[test]
fn skip_optional_dependency_that_does_not_support_the_current_node_version() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    write_manifest(
        &workspace,
        &serde_json::json!({
            "optionalDependencies": { "@pnpm.e2e/for-legacy-node": "*" },
        }),
    );

    pacquet.with_arg("install").assert().success();

    assert!(
        !workspace.join("node_modules/@pnpm.e2e/for-legacy-node").exists(),
        "an optional dependency for a legacy Node version must not be linked",
    );
    assert_eq!(read_skipped(&workspace), ["@pnpm.e2e/for-legacy-node@1.0.0"]);

    drop((root, npmrc_info)); // cleanup
}

/// TS: `do not skip optional dependency that does not support the current
/// pnpm version` (`optionalDependencies.ts:169`). `engines.pnpm` must not
/// make an optional dependency uninstallable.
#[test]
fn do_not_skip_optional_dependency_that_does_not_support_the_current_pnpm_version() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    write_manifest(
        &workspace,
        &serde_json::json!({
            "optionalDependencies": { "@pnpm.e2e/for-legacy-pnpm": "*" },
        }),
    );

    pacquet.with_arg("install").assert().success();

    assert!(
        workspace.join("node_modules/@pnpm.e2e/for-legacy-pnpm/package.json").exists(),
        "an optional dependency constrained on a legacy pnpm must still install",
    );
    assert_eq!(read_skipped(&workspace), Vec::<String>::new());

    drop((root, npmrc_info)); // cleanup
}

mod known_failures {
    //! Optional-dependency cases blocked on CLI surface pacquet hasn't
    //! built yet. Each entry stubs the not-yet-built subject under test
    //! through [`pacquet_testing_utils::allow_known_failure`] so the
    //! test exits early rather than masking a real bug.

    use pacquet_testing_utils::{
        allow_known_failure,
        known_failure::{KnownFailure, KnownResult},
    };

    fn install_force_flag() -> KnownResult<()> {
        Err(KnownFailure::new(
            "`pnpm install --force` is not wired on pacquet's install \
             command yet. `Config::force` exists and the installability \
             check honors it (`pnpm deploy --force` uses that path), but \
             the install/add CLI does not expose the flag, so the \
             force-installs-incompatible-optionals behavior is \
             unreachable end to end.",
        ))
    }

    /// TS: `don't skip optional dependency that does not support the
    /// current OS when forcing` (`optionalDependencies.ts:199`).
    #[test]
    fn do_not_skip_unsupported_os_optional_dependency_when_forcing() {
        allow_known_failure!(install_force_flag());
    }

    /// The forced-headless tail of TS `optional subdependency is skipped`
    /// (`optionalDependencies.ts:283`): `force: true, frozenLockfile: true`
    /// must materialize the incompatible optional and clear
    /// `.modules.yaml.skipped`.
    #[test]
    fn forced_frozen_install_materializes_incompatible_optionals() {
        allow_known_failure!(install_force_flag());
    }
}

/// TS: `skip optional dependency that does not support the current OS`
/// (`optionalDependencies.ts:74`). The full flow: skip on install, keep the
/// entries in both lockfiles, record the skip in `.modules.yaml`, restore a
/// previously-skipped package when it becomes a regular dependency, and
/// keep the skip set across a frozen reinstall.
#[test]
fn skip_optional_dependency_that_does_not_support_the_current_os() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    write_manifest(
        &workspace,
        &serde_json::json!({
            "optionalDependencies": { "@pnpm.e2e/not-compatible-with-any-os": "*" },
        }),
    );

    pacquet.with_arg("install").assert().success();

    assert!(
        !workspace.join("node_modules/@pnpm.e2e/not-compatible-with-any-os").exists(),
        "the platform-incompatible optional dependency must not be linked",
    );
    assert!(
        !workspace.join("node_modules/.pnpm/@pnpm.e2e+dep-of-optional-pkg@1.0.0").exists(),
        "the dependency of the skipped optional must not be materialized",
    );

    let lockfile = read_wanted_lockfile(&workspace);
    let packages = lockfile.packages.as_ref().expect("lockfile has packages");
    for name in
        ["@pnpm.e2e/not-compatible-with-any-os@1.0.0", "@pnpm.e2e/dep-of-optional-pkg@1.0.0"]
    {
        assert!(
            packages.keys().any(|key| key.to_string() == name),
            "the wanted lockfile must keep {name}",
        );
    }
    let current = read_current_lockfile(&workspace);
    let current_packages = current.packages.as_ref().expect("current lockfile has packages");
    assert_eq!(
        sorted_keys(current_packages),
        sorted_keys(packages),
        "the current lockfile must keep the skipped packages' metadata",
    );

    assert_eq!(
        read_skipped(&workspace),
        ["@pnpm.e2e/dep-of-optional-pkg@1.0.0", "@pnpm.e2e/not-compatible-with-any-os@1.0.0"],
    );

    // A previously skipped package is installed once it also becomes a
    // regular dependency.
    pacquet_in(&workspace).with_args(["add", "@pnpm.e2e/dep-of-optional-pkg"]).assert().success();
    assert!(
        workspace.join("node_modules/@pnpm.e2e/dep-of-optional-pkg/package.json").exists(),
        "the package must be installed once it is a regular dependency",
    );
    assert_eq!(read_skipped(&workspace), ["@pnpm.e2e/not-compatible-with-any-os@1.0.0"]);

    // The skip set survives a frozen reinstall from scratch.
    fs::remove_dir_all(workspace.join("node_modules")).expect("remove node_modules");
    pacquet_in(&workspace).with_args(["install", "--frozen-lockfile"]).assert().success();

    assert!(!workspace.join("node_modules/@pnpm.e2e/not-compatible-with-any-os").exists());
    assert!(workspace.join("node_modules/@pnpm.e2e/dep-of-optional-pkg/package.json").exists());
    assert_eq!(read_skipped(&workspace), ["@pnpm.e2e/not-compatible-with-any-os@1.0.0"]);

    drop((root, npmrc_info)); // cleanup
}

fn sorted_keys<Key: ToString, Value>(map: &std::collections::HashMap<Key, Value>) -> Vec<String> {
    let mut keys: Vec<String> = map.keys().map(ToString::to_string).collect();
    keys.sort();
    keys
}

/// TS: `optional subdependency is skipped` (`optionalDependencies.ts:283`),
/// minus the forced-headless tail (gated in [`known_failures`] on the
/// missing `install --force` flag).
#[test]
fn optional_subdependency_is_skipped() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();

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
    pacquet_in(&workspace).with_arg("install").assert().success();

    let lockfile = read_wanted_lockfile(&workspace);
    let packages = lockfile.packages.as_ref().expect("lockfile has packages");
    assert_eq!(packages.len(), 3, "packages: {:?}", sorted_keys(packages));
    assert!(
        packages.keys().any(|key| key.to_string() == "@pnpm.e2e/not-compatible-with-any-os@1.0.0"),
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
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();

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
    pacquet_in(&workspace).with_args(["install", "--frozen-lockfile"]).assert().success();

    assert_eq!(read_skipped(&workspace), Vec::<String>::new());

    drop((root, npmrc_info)); // cleanup
}

/// TS: `install optional dependency for the supported architecture set by
/// the user (nodeLinker=%s)` (`optionalDependencies.ts:594`). The
/// `--os` / `--cpu` overrides pick which platform-specific optional is
/// installed, across the fresh, non-frozen-rewrite, and frozen paths.
#[test]
fn install_optional_dependency_for_the_supported_architectures() {
    for node_linker in ["isolated", "hoisted"] {
        let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
            CommandTempCwd::init().add_mocked_registry();
        append_workspace_yaml_key(&workspace, "nodeLinker", node_linker);

        // Upstream verifies with `deepRequireCwd` — Node resolution
        // through the dependent, which the `.pnpm/node_modules` fallback
        // also satisfies. The port asserts the equivalent: the slot is
        // materialized and reachable through that resolution fallback.
        let installed_platform_dep = |name: &str| -> bool {
            if node_linker == "hoisted" {
                workspace.join("node_modules/@pnpm.e2e").join(name).join("package.json").exists()
            } else {
                workspace
                    .join(format!("node_modules/.pnpm/@pnpm.e2e+{name}@1.0.0"))
                    .join("node_modules/@pnpm.e2e")
                    .join(name)
                    .join("package.json")
                    .exists()
                    && workspace
                        .join("node_modules/.pnpm/node_modules/@pnpm.e2e")
                        .join(name)
                        .join("package.json")
                        .exists()
            }
        };

        pacquet
            .with_args([
                "add",
                "@pnpm.e2e/has-many-optional-deps@1.0.0",
                "--os",
                "darwin",
                "--cpu",
                "arm64",
            ])
            .assert()
            .success();
        assert!(installed_platform_dep("darwin-arm64"), "nodeLinker={node_linker}");
        assert!(!installed_platform_dep("darwin-x64"), "nodeLinker={node_linker}");

        pacquet_in(&workspace)
            .with_args(["install", "--no-prefer-frozen-lockfile", "--os", "darwin", "--cpu", "x64"])
            .assert()
            .success();
        assert!(installed_platform_dep("darwin-x64"), "nodeLinker={node_linker}");

        pacquet_in(&workspace)
            .with_args(["install", "--frozen-lockfile", "--os", "linux", "--cpu", "x64"])
            .assert()
            .success();
        assert!(installed_platform_dep("linux-x64"), "nodeLinker={node_linker}");

        drop((root, npmrc_info)); // cleanup
    }
}

/// TS: `not installing optional deps` (`deps-restorer/test/index.ts:323`):
/// a headless install with the optional group excluded skips the optional
/// dependency entirely — as the root's own optional and as the transitive
/// optional of `pkg-with-good-optional` — while regular dependencies
/// install.
#[test]
fn headless_install_without_optional_deps() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    write_manifest(
        &workspace,
        &serde_json::json!({
            "dependencies": { "@pnpm.e2e/pkg-with-good-optional": "1.0.0" },
            "optionalDependencies": { "is-positive": "1.0.0" },
        }),
    );

    pacquet.with_args(["install", "--lockfile-only"]).assert().success();
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
/// false, devDependencies: false, optionalDependencies: true }`; the
/// CLI-reachable equivalent is `--dev` (keep dev + optional, drop
/// production), which exercises the same headless include filtering.
#[test]
fn headless_install_include_filtering_excludes_production_group() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    write_manifest(
        &workspace,
        &serde_json::json!({
            "dependencies": { "@pnpm.e2e/foo": "^100.0.0" },
            "devDependencies": { "@pnpm.e2e/bar": "^100.0.0" },
            "optionalDependencies": { "@pnpm.e2e/qar": "^100.0.0" },
        }),
    );

    pacquet.with_args(["install", "--lockfile-only"]).assert().success();
    pacquet_in(&workspace).with_args(["install", "--frozen-lockfile", "--dev"]).assert().success();

    assert!(
        !workspace.join("node_modules/@pnpm.e2e/foo").exists(),
        "the excluded production dependency must not be linked",
    );
    assert!(
        workspace.join("node_modules/@pnpm.e2e/bar/package.json").exists(),
        "the dev dependency must be installed",
    );
    assert!(
        workspace.join("node_modules/@pnpm.e2e/qar/package.json").exists(),
        "the optional dependency must be installed",
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
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
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

    pacquet.with_args(["install", "--lockfile-only"]).assert().success();

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

    pacquet_in(&workspace).with_args(["install", "--frozen-lockfile"]).assert().success();

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

    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    append_workspace_yaml_key(&workspace, "packageImportMethod", "hardlink");
    write_manifest(
        &workspace,
        &serde_json::json!({
            "dependencies": { "@pnpm.e2e/pkg-with-good-optional": "*" },
        }),
    );

    let assert_hardlinked = || {
        let file = workspace
            .join("node_modules/.pnpm/is-positive@1.0.0/node_modules/is-positive/package.json");
        let metadata = fs::metadata(&file).expect("materialized optional dependency file exists");
        assert!(
            metadata.nlink() > 1,
            "the optional dependency must be hardlinked to the store (nlink = {})",
            metadata.nlink(),
        );
    };

    pacquet.with_arg("install").assert().success();
    assert_hardlinked();

    fs::remove_dir_all(workspace.join("node_modules")).expect("remove node_modules");
    pacquet_in(&workspace).with_args(["install", "--frozen-lockfile"]).assert().success();
    assert_hardlinked();

    drop((root, npmrc_info)); // cleanup
}
