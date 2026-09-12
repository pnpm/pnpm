//! End-to-end coverage for the hoisting pass.
//!
//! Each test sets up a workspace with a `package.json`, lets pnpm
//! generate the lockfile against the registry mock, then runs
//! `pacquet install --frozen-lockfile`. The frozen-lockfile path is
//! the only path that runs the hoist algorithm — without-lockfile
//! returns an empty `HoistedDependencies` map by design (no graph to
//! walk; the lockfile is the source of the snapshot graph).
//!
//! The pnpm-driven setup keeps integrity hashes in sync with whatever
//! the registry mock serves; pre-baking a tiny v9 lockfile by hand
//! would diverge silently the moment a fixture changes.
//!
//! Every ported hoist case runs against the real implementation; the
//! suite has no `known_failures` stubs.
//!
//! Workspace install (pnpm/pacquet#431) landed in [#443]. The
//! [`workspace_hoist_walks_every_importer`] test below covers the
//! basic multi-importer case; `hoistWorkspacePackages` name-links are
//! covered by [`hoist_workspace_packages_links_projects_by_name`].
//!
//! [#443]: https://github.com/pnpm/pacquet/pull/443

#![cfg(unix)] // pnpm CLI: 'program not found' on Windows runners.

pub use _utils::*;

use crate::_utils;

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::{
    bin::{AddMockedRegistry, CommandTempCwd},
    fs::{bump_mtime, is_symlink_or_junction},
};
use std::{fs, path::Path, process::Command};

/// Generate a `pnpm-lock.yaml` in `workspace` against the mocked
/// registry, without installing anything. Pacquet then consumes that
/// lockfile via `--frozen-lockfile` to drive the hoist pass.
fn generate_lockfile(pnpm: Command) {
    pnpm.with_args(["install", "--lockfile-only", "--ignore-scripts"]).assert().success();
}

/// Replace the boilerplate `pnpm-workspace.yaml` written by
/// `add_mocked_registry` with one that adds `hoistPattern` /
/// `publicHoistPattern` overrides on top of the same `storeDir` /
/// `cacheDir`. Anchored against the mock locations so the original
/// `add_mocked_registry` `.npmrc` continues to work.
fn write_workspace_yaml(workspace: &Path, extra: &str) {
    let yaml = format!("storeDir: ../pacquet-store\ncacheDir: ../pacquet-cache\n{extra}");
    fs::write(workspace.join("pnpm-workspace.yaml"), yaml).expect("write pnpm-workspace.yaml");
}

/// Write a one-dependency `package.json` and return the manifest path.
#[expect(
    clippy::needless_pass_by_value,
    reason = "test helper called many times with json!(...) literals; owned arg keeps call sites clean"
)]
fn write_manifest(workspace: &Path, deps: serde_json::Value) {
    let manifest = serde_json::json!({ "dependencies": deps });
    fs::write(workspace.join("package.json"), manifest.to_string()).expect("write package.json");
}

/// TS: `hoisting should not create a broken symlink to a skipped optional
/// dependency` (`hoist.ts:540`): with `publicHoistPattern: '*'`, neither
/// the skipped platform-incompatible optional nor its dependency may
/// appear — as a working or dangling symlink — at either hoist target, on
/// the fresh install and on the frozen reinstall.
#[test]
fn hoisting_skips_broken_symlink_for_skipped_optional() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_workspace_yaml(
        &workspace,
        "enableGlobalVirtualStore: false\npublicHoistPattern:\n  - '*'\n",
    );
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "dependencies": { "is-positive": "1.0.0" },
            "optionalDependencies": { "@pnpm.e2e/not-compatible-with-any-os": "*" },
        })
        .to_string(),
    )
    .expect("write package.json");

    let assert_no_broken_hoist_links = |phase: &str| {
        for hoist_dir in ["node_modules/@pnpm.e2e", "node_modules/.pnpm/node_modules/@pnpm.e2e"] {
            for name in ["dep-of-optional-pkg", "not-compatible-with-any-os"] {
                let path = workspace.join(hoist_dir).join(name);
                assert!(
                    fs::symlink_metadata(&path).is_err(),
                    "[{phase}] no symlink (dangling or otherwise) may be created for the \
                     skipped optional subtree at {path:?}; .modules.yaml: {}",
                    fs::read_to_string(workspace.join("node_modules/.modules.yaml"))
                        .unwrap_or_default(),
                );
            }
        }
    };

    pacquet.with_arg("install").assert().success();
    assert_no_broken_hoist_links("fresh");

    fs::remove_dir_all(workspace.join("node_modules")).expect("remove node_modules");
    Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(&workspace)
        .with_args(["install", "--frozen-lockfile"])
        .assert()
        .success();
    assert_no_broken_hoist_links("frozen");

    drop((root, mock_instance));
}

/// `publicHoistPattern: ["*"]` hoists every transitive into the
/// project's root `node_modules/`.
#[test]
fn public_hoist_star_hoists_to_root_node_modules() {
    let CommandTempCwd { pacquet, pnpm, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_manifest(
        &workspace,
        serde_json::json!({ "@pnpm.e2e/hello-world-js-bin-parent": "1.0.0" }),
    );
    generate_lockfile(pnpm);
    write_workspace_yaml(&workspace, "publicHoistPattern:\n  - '*'\nhoistPattern: []\n");

    pacquet.with_args(["install", "--frozen-lockfile"]).assert().success();

    let public_hoist = workspace.join("node_modules/@pnpm.e2e/hello-world-js-bin");
    assert!(
        is_symlink_or_junction(&public_hoist).unwrap(),
        "transitive should be publicly hoisted at {public_hoist:?}",
    );
    assert!(
        !workspace.join("node_modules/.pnpm/node_modules/@pnpm.e2e/hello-world-js-bin").exists(),
        "transitive should not be privately hoisted when private pattern is empty",
    );

    drop((root, mock_instance));
}

#[test]
fn public_hoist_does_not_override_an_existing_root_directory() {
    let CommandTempCwd { pacquet, pnpm, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_manifest(
        &workspace,
        serde_json::json!({ "@pnpm.e2e/hello-world-js-bin-parent": "1.0.0" }),
    );
    generate_lockfile(pnpm);
    write_workspace_yaml(&workspace, "publicHoistPattern:\n  - '*'\nhoistPattern: []\n");
    let occupied = workspace.join("node_modules/@pnpm.e2e/hello-world-js-bin");
    fs::create_dir_all(&occupied).expect("create occupied public-hoist slot");
    fs::write(occupied.join("keep.txt"), "external").expect("write marker");

    pacquet.with_args(["install", "--frozen-lockfile"]).assert().success();

    assert_eq!(fs::read_to_string(occupied.join("keep.txt")).unwrap(), "external");
    assert!(!is_symlink_or_junction(&occupied).unwrap());

    drop((root, mock_instance));
}

/// `.modules.yaml` records `hoistedDependencies` when the hoist pass
/// runs.
#[test]
fn modules_yaml_records_hoisted_dependencies() {
    let CommandTempCwd { pacquet, pnpm, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_manifest(
        &workspace,
        serde_json::json!({ "@pnpm.e2e/hello-world-js-bin-parent": "1.0.0" }),
    );
    generate_lockfile(pnpm);

    pacquet.with_args(["install", "--frozen-lockfile"]).assert().success();

    let modules_yaml_text = fs::read_to_string(workspace.join("node_modules/.modules.yaml"))
        .expect("read .modules.yaml");
    // The manifest is JSON-with-quoted-keys (pnpm's chosen format),
    // so a substring match for the depPath + alias is sufficient and
    // doesn't drag in a YAML parser. Assert the presence rather than
    // exact serialization to keep the test resilient to ordering.
    assert!(
        modules_yaml_text.contains(r#""@pnpm.e2e/hello-world-js-bin@1.0.0""#),
        "hoistedDependencies should record the transitive dep path; got:\n{modules_yaml_text}",
    );
    // Alias-as-stored is the full scoped name, since that's how the
    // dep appears in `@pnpm.e2e/hello-world-js-bin-parent`'s
    // `dependencies` map. The record is keyed by dep path then alias,
    // mapping to the hoist kind.
    assert!(
        modules_yaml_text.contains(r#""@pnpm.e2e/hello-world-js-bin": "private""#),
        "transitive should be marked as `private` hoist; got:\n{modules_yaml_text}",
    );

    drop((root, mock_instance));
}

/// Privately-hoisted bins land in `<vs>/node_modules/.bin/`: the
/// hoisted bin appears at
/// `.pnpm/node_modules/.bin/hello-world-js-bin`.
#[test]
fn private_hoist_links_bins() {
    let CommandTempCwd { pacquet, pnpm, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_manifest(
        &workspace,
        serde_json::json!({ "@pnpm.e2e/hello-world-js-bin-parent": "1.0.0" }),
    );
    generate_lockfile(pnpm);

    pacquet.with_args(["install", "--frozen-lockfile"]).assert().success();

    let bin_path = workspace.join("node_modules/.pnpm/node_modules/.bin/hello-world-js-bin");
    assert!(bin_path.exists(), "private hoist should link bin at {bin_path:?}");

    drop((root, mock_instance));
}

/// Public hoist of an alias whose target package declares a bin —
/// the bin should land in `<root>/node_modules/.bin/` (linked by the
/// existing direct-deps bin pass, not by [`link_hoisted_bins`]).
#[test]
fn public_hoist_bin_is_linked_via_root_bin_dir() {
    let CommandTempCwd { pacquet, pnpm, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_manifest(
        &workspace,
        serde_json::json!({ "@pnpm.e2e/hello-world-js-bin-parent": "1.0.0" }),
    );
    generate_lockfile(pnpm);
    write_workspace_yaml(&workspace, "publicHoistPattern:\n  - '*'\nhoistPattern: []\n");

    pacquet.with_args(["install", "--frozen-lockfile"]).assert().success();

    let alias_link = workspace.join("node_modules/@pnpm.e2e/hello-world-js-bin");
    assert!(
        is_symlink_or_junction(&alias_link).unwrap(),
        "public hoist should link the alias under `<root>/node_modules/`",
    );
    // Pacquet's pipeline runs `SymlinkDirectDependencies` *before* the
    // hoist pass, so the install path makes a second
    // `link_direct_dep_bins` call against the public-hoisted aliases
    // after the symlinks are in place.
    let bin_path = workspace.join("node_modules/.bin/hello-world-js-bin");
    assert!(bin_path.exists(), "public hoist should link bin at {bin_path:?}");

    drop((root, mock_instance));
}

/// `nodeLinker: hoisted` on the fresh-lockfile path (no checked-in
/// lockfile, no `--frozen-lockfile`) must lay every dependency out as
/// a **real directory** flat under the project's `node_modules/`, not
/// as a symlink into a `.pnpm` virtual store. Closes
/// [#11871](https://github.com/pnpm/pnpm/issues/11871): the fresh
/// path used to hard-refuse the combination.
///
/// Uses `@pnpm.e2e/hello-world-js-bin-parent` (a direct dep) which
/// pulls in `@pnpm.e2e/hello-world-js-bin` as a transitive — under
/// the hoisted linker both land at the top level as real dirs.
#[test]
fn fresh_install_hoisted_node_linker_lands_real_directories() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_manifest(
        &workspace,
        serde_json::json!({ "@pnpm.e2e/hello-world-js-bin-parent": "1.0.0" }),
    );
    write_workspace_yaml(&workspace, "nodeLinker: hoisted\n");

    // No `generate_lockfile` and no `--frozen-lockfile`: this drives
    // the fresh-resolve path.
    pacquet.with_args(["install"]).assert().success();

    let is_real_dir = |relative: &str| -> bool {
        let path = workspace.join(relative);
        path.is_dir() && !is_symlink_or_junction(&path).unwrap()
    };

    assert!(
        is_real_dir("node_modules/@pnpm.e2e/hello-world-js-bin-parent"),
        "direct dep should be a real directory under node_modules/, not a symlink",
    );
    assert!(
        workspace.join("node_modules/@pnpm.e2e/hello-world-js-bin-parent/package.json").is_file(),
        "real directory should contain the package's package.json",
    );
    assert!(
        is_real_dir("node_modules/@pnpm.e2e/hello-world-js-bin"),
        "transitive dep should be hoisted to a real directory at the top level",
    );
    // The hoisted linker writes no virtual-store slot directories.
    // (`node_modules/.pnpm` itself may exist to hold the current
    // `lock.yaml`, matching pnpm, but no per-package slot is laid
    // down — that's the isolated linker's shape.)
    assert!(
        !workspace.join("node_modules/.pnpm/@pnpm.e2e+hello-world-js-bin@1.0.0").exists(),
        "hoisted linker must not materialize a virtual-store slot for the package",
    );
    assert!(
        !workspace.join("node_modules/.pnpm/node_modules").exists(),
        "hoisted linker must not create a private-hoist `.pnpm/node_modules` tree",
    );
    assert!(
        workspace.join("node_modules/.bin/hello-world-js-bin").exists(),
        "hoisted linker should link the transitive's bin into node_modules/.bin",
    );
    assert!(
        workspace.join("pnpm-lock.yaml").is_file(),
        "fresh install should write a wanted lockfile",
    );

    drop((root, mock_instance));
}

/// The `hoistedDependencies` map from `node_modules/.modules.yaml`.
fn hoisted_dependencies(workspace: &Path) -> serde_json::Value {
    let text = fs::read_to_string(workspace.join("node_modules/.modules.yaml"))
        .expect("read .modules.yaml");
    let manifest: serde_json::Value =
        serde_json::from_str(&text).expect(".modules.yaml holds JSON content");
    manifest["hoistedDependencies"].clone()
}

/// `version` field of the `package.json` under `dir`.
fn version_of(dir: &Path) -> String {
    let text = fs::read_to_string(dir.join("package.json")).expect("read package.json");
    let manifest: serde_json::Value = serde_json::from_str(&text).expect("parse package.json");
    manifest["version"].as_str().expect("version is a string").to_string()
}

/// TS: `should hoist dependencies` (`hoist.ts:24`), the repeat-install
/// tail: `hoistedDependencies` must come out identical after a
/// re-resolving repeat install and after a frozen re-materialization.
#[test]
fn should_hoist_dependencies_repeat_install_preserves_map() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    pacquet.with_args(["add", "@pnpm.e2e/foobarqar@1.0.0"]).assert().success();
    let baseline = hoisted_dependencies(&workspace);
    eprintln!("baseline hoistedDependencies: {baseline:#}");
    assert!(
        baseline.as_object().is_some_and(|map| !map.is_empty()),
        "transitives must be hoisted on the first install",
    );

    // Re-write the manifest (same bytes, fresh mtime) so the repeat
    // install takes the full pipeline instead of the optimistic
    // repeat-install short-circuit.
    let manifest_path = workspace.join("package.json");
    let manifest_bytes = fs::read(&manifest_path).expect("read package.json");
    fs::write(&manifest_path, manifest_bytes).expect("rewrite package.json");
    bump_mtime(&manifest_path);
    pacquet_in(&workspace).with_arg("install").assert().success();
    assert!(workspace.join("node_modules/.pnpm/node_modules/@pnpm.e2e/foo").exists());
    assert_eq!(hoisted_dependencies(&workspace), baseline);

    fs::remove_dir_all(workspace.join("node_modules")).expect("remove node_modules");
    pacquet_in(&workspace).with_args(["install", "--frozen-lockfile"]).assert().success();
    assert!(workspace.join("node_modules/.pnpm/node_modules/@pnpm.e2e/foo").exists());
    assert_eq!(hoisted_dependencies(&workspace), baseline);

    drop((root, mock_instance));
}

/// TS: `should remove hoisted dependencies` (`hoist.ts:121`):
/// removing the dependency that owned the hoisted transitives removes
/// their hoist links too.
#[test]
fn should_remove_hoisted_dependencies() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    pacquet.with_args(["add", "@pnpm.e2e/pkg-with-1-dep@100.0.0"]).assert().success();
    let hoisted = workspace.join("node_modules/.pnpm/node_modules/@pnpm.e2e/dep-of-pkg-with-1-dep");
    assert!(is_symlink_or_junction(&hoisted).expect("check hoist link"));

    pacquet_in(&workspace).with_args(["remove", "@pnpm.e2e/pkg-with-1-dep"]).assert().success();
    assert!(!workspace.join("node_modules/@pnpm.e2e/pkg-with-1-dep").exists());
    assert!(
        fs::symlink_metadata(&hoisted).is_err(),
        "the transitive's hoist link must be removed with its owner",
    );
    assert_eq!(hoisted_dependencies(&workspace), serde_json::json!({}));

    drop((root, mock_instance));
}

/// TS: `should not override root packages with hoisted dependencies`
/// (`hoist.ts:137`): a direct dependency keeps its slot even when a
/// transitive with the same alias and different version enters the
/// graph.
#[test]
fn should_not_override_root_packages_with_hoisted_deps() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    pacquet.with_args(["add", "@pnpm.e2e/bar@100.1.0"]).assert().success();
    pacquet_in(&workspace).with_args(["add", "@pnpm.e2e/foobarqar@1.0.0"]).assert().success();

    assert_eq!(version_of(&workspace.join("node_modules/@pnpm.e2e/bar")), "100.1.0");

    drop((root, mock_instance));
}

/// TS: `should rehoist when uninstalling a package` (`hoist.ts:148`):
/// removing the direct dep frees its alias for the transitive to be
/// hoisted.
#[test]
fn should_rehoist_when_uninstalling_a_package() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    pacquet
        .with_args(["add", "@pnpm.e2e/bar@100.1.0", "@pnpm.e2e/foobarqar@1.0.0"])
        .assert()
        .success();
    let hoisted_bar = workspace.join("node_modules/.pnpm/node_modules/@pnpm.e2e/bar");
    assert!(
        fs::symlink_metadata(&hoisted_bar).is_err(),
        "a root direct dep's alias must not be hoisted",
    );

    pacquet_in(&workspace).with_args(["remove", "@pnpm.e2e/bar"]).assert().success();
    assert!(
        !workspace.join("node_modules/@pnpm.e2e/bar").exists(),
        "the removed direct dep's root link must be gone",
    );
    assert_eq!(version_of(&hoisted_bar), "100.0.0");
    assert_eq!(version_of(&workspace.join("node_modules/@pnpm.e2e/foobarqar")), "1.0.0");
    let hoisted = hoisted_dependencies(&workspace);
    eprintln!("hoistedDependencies after remove: {hoisted:#}");
    assert_eq!(hoisted["@pnpm.e2e/bar@100.0.0"], serde_json::json!({ "@pnpm.e2e/bar": "private" }));

    drop((root, mock_instance));
}

/// TS: `should rehoist after running a general install` (`hoist.ts:169`):
/// dropping the direct dep from the manifest and re-installing hoists
/// the transitive, without recreating the untouched direct-dep links.
#[test]
fn should_rehoist_after_running_a_general_install() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_manifest(
        &workspace,
        serde_json::json!({ "@pnpm.e2e/bar": "100.1.0", "@pnpm.e2e/foobarqar": "1.0.0" }),
    );
    pacquet.with_arg("install").assert().success();
    let foobarqar_link = workspace.join("node_modules/@pnpm.e2e/foobarqar");
    assert_eq!(version_of(&workspace.join("node_modules/@pnpm.e2e/bar")), "100.1.0");
    let prev_target = fs::canonicalize(&foobarqar_link).expect("resolve foobarqar link");
    assert!(
        fs::symlink_metadata(workspace.join("node_modules/.pnpm/node_modules/@pnpm.e2e/bar"))
            .is_err(),
        "a root direct dep's alias must not be hoisted",
    );

    write_manifest(&workspace, serde_json::json!({ "@pnpm.e2e/foobarqar": "1.0.0" }));
    pacquet_in(&workspace).with_arg("install").assert().success();
    let curr_target = fs::canonicalize(&foobarqar_link).expect("resolve foobarqar link");
    assert_eq!(prev_target, curr_target, "the untouched direct dep keeps its link target");
    assert_eq!(
        version_of(&workspace.join("node_modules/.pnpm/node_modules/@pnpm.e2e/bar")),
        "100.0.0",
    );

    drop((root, mock_instance));
}

/// TS: `should not override aliased dependencies` (`hoist.ts:201`): a
/// root alias wins over a transitive that hoists under the same alias.
#[test]
fn should_not_override_aliased_dependencies() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    pacquet
        .with_args(["add", "dep@npm:is-positive@1.0.0", "@pnpm.e2e/pkg-with-1-aliased-dep"])
        .assert()
        .success();

    assert_eq!(version_of(&workspace.join("node_modules/dep")), "1.0.0");

    drop((root, mock_instance));
}

/// TS: `hoist by alias` (`hoist.ts:233`): an npm-aliased transitive is
/// hoisted under its alias, not its real name, and `.modules.yaml`
/// records the alias.
#[test]
fn hoist_by_alias() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    pacquet.with_args(["add", "@pnpm.e2e/pkg-with-1-aliased-dep@100.0.0"]).assert().success();

    assert!(workspace.join("node_modules/@pnpm.e2e/pkg-with-1-aliased-dep").exists());
    assert!(workspace.join("node_modules/.pnpm/node_modules/dep").exists());
    assert!(
        fs::symlink_metadata(
            workspace.join("node_modules/.pnpm/node_modules/@pnpm.e2e/dep-of-pkg-with-1-dep"),
        )
        .is_err(),
        "the aliased dep must be hoisted under its alias only",
    );
    assert_eq!(
        hoisted_dependencies(&workspace),
        serde_json::json!({ "@pnpm.e2e/dep-of-pkg-with-1-dep@100.1.0": { "dep": "private" } }),
    );

    drop((root, mock_instance));
}

/// TS: `should remove aliased hoisted dependencies` (`hoist.ts:249`).
#[test]
fn should_remove_aliased_hoisted_dependencies() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    pacquet.with_args(["add", "@pnpm.e2e/pkg-with-1-aliased-dep@100.0.0"]).assert().success();
    pacquet_in(&workspace)
        .with_args(["remove", "@pnpm.e2e/pkg-with-1-aliased-dep"])
        .assert()
        .success();

    assert!(!workspace.join("node_modules/@pnpm.e2e/pkg-with-1-aliased-dep").exists());
    assert!(
        fs::symlink_metadata(workspace.join("node_modules/.pnpm/node_modules/dep")).is_err(),
        "the aliased hoist link must be removed with its owner",
    );
    assert_eq!(hoisted_dependencies(&workspace), serde_json::json!({}));

    drop((root, mock_instance));
}

/// TS: `should update .modules.yaml when pruning if we are flattening`
/// (`hoist.ts:272`): pruning to an empty manifest clears
/// `hoistedDependencies`.
#[test]
fn modules_yaml_updated_on_prune_when_flattening() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_manifest(&workspace, serde_json::json!({ "@pnpm.e2e/pkg-with-1-aliased-dep": "*" }));
    pacquet.with_arg("install").assert().success();
    assert!(workspace.join("node_modules/.pnpm/node_modules/dep").exists());

    write_manifest(&workspace, serde_json::json!({}));
    pacquet_in(&workspace).with_arg("install").assert().success();
    assert_eq!(hoisted_dependencies(&workspace), serde_json::json!({}));
    assert!(
        fs::symlink_metadata(workspace.join("node_modules/.pnpm/node_modules/dep")).is_err(),
        "pruning to an empty manifest must drop the hoist link",
    );

    drop((root, mock_instance));
}

/// TS: `should rehoist after pruning` (`hoist.ts:288`): same shape as
/// the general-install rehoist, with an unrelated dep added in the
/// same step.
#[test]
fn should_rehoist_after_pruning() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_manifest(
        &workspace,
        serde_json::json!({ "@pnpm.e2e/bar": "100.1.0", "@pnpm.e2e/foobarqar": "1.0.0" }),
    );
    pacquet.with_arg("install").assert().success();
    let foobarqar_link = workspace.join("node_modules/@pnpm.e2e/foobarqar");
    let prev_target = fs::canonicalize(&foobarqar_link).expect("resolve foobarqar link");

    write_manifest(
        &workspace,
        serde_json::json!({ "@pnpm.e2e/foobarqar": "1.0.0", "is-positive": "1.0.0" }),
    );
    pacquet_in(&workspace).with_arg("install").assert().success();
    let curr_target = fs::canonicalize(&foobarqar_link).expect("resolve foobarqar link");
    assert_eq!(prev_target, curr_target, "the untouched direct dep keeps its link target");
    assert_eq!(
        version_of(&workspace.join("node_modules/.pnpm/node_modules/@pnpm.e2e/bar")),
        "100.0.0",
    );

    drop((root, mock_instance));
}

/// TS: `should hoist correctly peer dependencies` (`hoist.ts:320`):
/// `@pnpm.e2e/using-ajv` depends on both `ajv` and `ajv-keywords`;
/// `ajv-keywords`'s peer `ajv` resolves to the sibling, producing the
/// peer-variant snapshot `ajv-keywords@1.5.0(ajv@4.10.4)`, which must
/// be the target of the private hoist link.
#[test]
fn should_hoist_correctly_peer_dependencies() {
    let CommandTempCwd { pacquet, pnpm, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_workspace_yaml(
        &workspace,
        "enableGlobalVirtualStore: false\nhoistPattern:\n  - '*'\nautoInstallPeers: true\n",
    );
    write_manifest(&workspace, serde_json::json!({ "@pnpm.e2e/using-ajv": "1.0.0" }));
    generate_lockfile(pnpm);

    pacquet.with_args(["install", "--frozen-lockfile"]).assert().success();

    let hoisted = workspace.join("node_modules/.pnpm/node_modules/ajv-keywords");
    assert!(
        is_symlink_or_junction(&hoisted).unwrap(),
        "`ajv-keywords` must be privately hoisted at {hoisted:?}",
    );
    let variant_dir = workspace
        .join("node_modules/.pnpm/ajv-keywords@1.5.0_ajv@4.10.4/node_modules/ajv-keywords");
    assert!(variant_dir.is_dir(), "peer-variant slot missing at {variant_dir:?}");
    assert_eq!(
        fs::canonicalize(&hoisted).unwrap(),
        fs::canonicalize(&variant_dir).unwrap(),
        "the hoist link must resolve to the peer-variant slot",
    );

    drop((root, mock_instance));
}

/// TS: `should uninstall correctly peer dependencies` (`hoist.ts:327`):
/// dropping `@pnpm.e2e/using-ajv` from the manifest and re-installing
/// removes the peer-variant hoist link.
#[test]
fn should_uninstall_correctly_peer_dependencies() {
    let CommandTempCwd { pacquet, pnpm, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_workspace_yaml(
        &workspace,
        "enableGlobalVirtualStore: false\nhoistPattern:\n  - '*'\nautoInstallPeers: true\n",
    );
    write_manifest(&workspace, serde_json::json!({ "@pnpm.e2e/using-ajv": "1.0.0" }));
    generate_lockfile(pnpm);
    pacquet.with_args(["install", "--frozen-lockfile"]).assert().success();
    assert!(
        is_symlink_or_junction(&workspace.join("node_modules/.pnpm/node_modules/ajv-keywords"))
            .unwrap(),
        "`ajv-keywords` must be privately hoisted before the uninstall",
    );

    write_manifest(&workspace, serde_json::json!({}));
    generate_lockfile(Command::new("pnpm").with_current_dir(&workspace));
    pacquet_in(&workspace).with_args(["install", "--frozen-lockfile"]).assert().success();

    assert!(
        fs::symlink_metadata(workspace.join("node_modules/ajv-keywords")).is_err(),
        "no symlink to the peer dep may remain in root node_modules",
    );
    assert!(
        fs::symlink_metadata(workspace.join("node_modules/.pnpm/node_modules/ajv-keywords"))
            .is_err(),
        "the peer-variant hoist link must be removed with its owner",
    );

    drop((root, mock_instance));
}

/// TS: `should recreate node_modules with hoisting` (`hoist.ts:514`):
/// a plain install may recreate a modules dir installed without
/// hoisting, and the recreated tree is hoisted.
#[test]
fn should_recreate_node_modules_with_hoisting() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_workspace_yaml(&workspace, "hoistPattern: []\n");
    pacquet.with_args(["add", "@pnpm.e2e/pkg-with-1-dep@100.0.0"]).assert().success();
    assert!(
        fs::symlink_metadata(
            workspace.join("node_modules/.pnpm/node_modules/@pnpm.e2e/dep-of-pkg-with-1-dep"),
        )
        .is_err(),
        "hoisting disabled: no hoist link may exist",
    );
    assert_eq!(hoisted_dependencies(&workspace), serde_json::json!({}));

    write_workspace_yaml(&workspace, "");
    pacquet_in(&workspace).with_arg("install").assert().success();
    assert!(workspace.join("node_modules/@pnpm.e2e/pkg-with-1-dep").exists());
    assert!(
        workspace.join("node_modules/.pnpm/node_modules/@pnpm.e2e/dep-of-pkg-with-1-dep").exists(),
    );
    assert!(
        hoisted_dependencies(&workspace).as_object().is_some_and(|map| !map.is_empty()),
        "the recreated tree must be hoisted",
    );

    drop((root, mock_instance));
}

/// TS: `the hoisted packages should not override the bin files of the
/// direct dependencies` (`install/hoist.ts:567`).
#[test]
fn hoisted_packages_dont_override_direct_dep_bins() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_manifest(
        &workspace,
        serde_json::json!({ "@pnpm.e2e/hello-world-js-bin-parent": "1.0.0" }),
    );
    write_workspace_yaml(&workspace, "publicHoistPattern:\n  - '*'\n");
    let assert_direct_bin_wins = || {
        let shim = fs::read_to_string(workspace.join("node_modules/.bin/hello-world-js-bin"))
            .expect("read hello-world-js-bin shim");
        assert!(
            shim.contains("hello-world-js-bin-parent"),
            "the direct dependency's bin must win over the publicly hoisted transitive:\n{shim}",
        );
    };

    pacquet.with_arg("install").assert().success();
    assert_direct_bin_wins();

    fs::remove_dir_all(workspace.join("node_modules")).expect("remove node_modules");
    pacquet_in(&workspace).with_args(["install", "--frozen-lockfile"]).assert().success();
    assert_direct_bin_wins();

    drop((root, mock_instance));
}

/// TS: `should add extra node paths to command shims` (`hoist.ts:790`).
#[test]
fn should_add_extra_node_paths_to_command_shims() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();

    pacquet.with_args(["add", "@pnpm.e2e/hello-world-js-bin"]).assert().success();

    let shim = fs::read_to_string(workspace.join("node_modules/.bin/hello-world-js-bin"))
        .expect("read the command shim");
    assert!(
        shim.contains("node_modules/.pnpm/node_modules"),
        "the shim must extend NODE_PATH with the hidden hoisted modules dir:\n{shim}",
    );

    // The fresh install's own `packages:` rows must record `hasBin` —
    // the bin linker's slot short-circuit trusts them on this very
    // install, before any save/load round-trip.
    let lockfile = read_lockfile(&workspace.join("pnpm-lock.yaml"));
    let packages = lockfile.packages.as_ref().expect("lockfile has packages");
    let (_, metadata) = packages
        .iter()
        .find(|(key, _)| key.to_string() == "@pnpm.e2e/hello-world-js-bin@1.0.0")
        .expect("lockfile records the added package");
    assert_eq!(metadata.has_bin, Some(true), "the fresh lockfile must record hasBin");

    drop((root, npmrc_info)); // cleanup
}

/// TS: `should not add extra node paths to command shims, when
/// extend-node-path is set to false` (`hoist.ts:799`).
#[test]
fn should_not_add_extra_node_paths_when_extend_node_path_false() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    append_workspace_yaml_key(&workspace, "extendNodePath", "false");

    pacquet.with_args(["add", "@pnpm.e2e/hello-world-js-bin"]).assert().success();

    let shim = fs::read_to_string(workspace.join("node_modules/.bin/hello-world-js-bin"))
        .expect("read the command shim");
    assert!(
        !shim.contains("node_modules/.pnpm/node_modules"),
        "`extendNodePath: false` must keep NODE_PATH out of the shim:\n{shim}",
    );

    drop((root, npmrc_info)); // cleanup
}

/// The lockfile-only resolve stands in for pulling a teammate's
/// lockfile; the install after it takes the headless path.
#[test]
fn rehoists_when_an_up_to_date_lockfile_removes_a_direct_dependency() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_workspace_yaml(&workspace, "hoistPattern:\n  - '*'\n");
    write_manifest(
        &workspace,
        serde_json::json!({
            "@pnpm.e2e/pkg-with-1-dep": "100.0.0",
            "@pnpm.e2e/dep-of-pkg-with-1-dep": "101.0.0",
        }),
    );
    pacquet.with_arg("install").assert().success();
    let hoisted = workspace.join("node_modules/.pnpm/node_modules/@pnpm.e2e/dep-of-pkg-with-1-dep");
    assert!(
        fs::symlink_metadata(&hoisted).is_err(),
        "a direct dependency's name is not privately hoisted",
    );

    write_manifest(&workspace, serde_json::json!({ "@pnpm.e2e/pkg-with-1-dep": "100.0.0" }));
    pacquet_at(&workspace).with_args(["install", "--lockfile-only"]).assert().success();
    pacquet_at(&workspace).with_arg("install").assert().success();

    assert!(
        is_symlink_or_junction(&hoisted).unwrap_or(false),
        "the transitive copy is hoisted once the direct dependency is gone",
    );
    let manifest: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(hoisted.join("package.json")).expect("read hoisted package.json"),
    )
    .expect("parse hoisted package.json");
    assert_eq!(manifest["version"], "100.1.0");

    drop((root, mock_instance));
}

fn pacquet_at(workspace: &Path) -> Command {
    Command::cargo_bin("pnpm").expect("find the pnpm binary").with_current_dir(workspace)
}

mod workspace;

mod patterns;
