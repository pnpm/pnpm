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
//! Test ports of upstream's
//! [`installing/deps-installer/test/install/hoist.ts`](https://github.com/pnpm/pnpm/blob/94240bc046/installing/deps-installer/test/install/hoist.ts)
//! that depend on features pacquet hasn't built yet (partial install
//! / re-hoist — pnpm/pacquet#433, GVS — pnpm/pacquet#432, peer-dep
//! details, hoisted node-linker, `hoistWorkspacePackages`,
//! `extendNodePath`) live in [`known_failures`] below with
//! [`pacquet_testing_utils::allow_known_failure`] gating the assertion
//! against the not-yet-implemented subject under test.
//!
//! Workspace install (pnpm/pacquet#431) landed in [#443]. The
//! [`workspace_hoist_walks_every_importer`] test below covers the
//! basic multi-importer case directly; the upstream
//! `hoistWorkspacePackages` test still lives under [`known_failures`]
//! because pacquet doesn't yet model the
//! `hoistedWorkspacePackages` shape itself (linking workspace
//! projects into the hoist target tree).
//!
//! [#443]: https://github.com/pnpm/pacquet/pull/443

#![cfg(unix)] // pnpm CLI: 'program not found' on Windows runners.

pub mod _utils;
pub use _utils::*;

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pacquet_testing_utils::{
    bin::{AddMockedRegistry, CommandTempCwd},
    fs::is_symlink_or_junction,
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

/// Default hoist patterns hoist every transitive into
/// `<vs>/node_modules/`. Mirrors upstream's
/// [`hoist.ts:24` "should hoist dependencies"](https://github.com/pnpm/pnpm/blob/94240bc046/installing/deps-installer/test/install/hoist.ts#L24).
/// Single-importer subset — the upstream test also re-runs install
/// twice to assert the persisted map is preserved; that requires
/// partial install (pnpm/pacquet#433) and lives in
/// [`known_failures::should_hoist_dependencies_repeat_install_preserves_map`].
#[test]
fn private_hoist_default_pattern_hoists_transitives() {
    let CommandTempCwd { pacquet, pnpm, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_manifest(
        &workspace,
        serde_json::json!({ "@pnpm.e2e/hello-world-js-bin-parent": "1.0.0" }),
    );
    generate_lockfile(pnpm);

    pacquet.with_args(["install", "--frozen-lockfile"]).assert().success();

    // Direct dep is at the root.
    assert!(
        is_symlink_or_junction(&workspace.join("node_modules/@pnpm.e2e/hello-world-js-bin-parent"))
            .unwrap(),
        "direct dep symlink missing",
    );
    // Transitive is privately hoisted.
    let private_hoist =
        workspace.join("node_modules/.pnpm/node_modules/@pnpm.e2e/hello-world-js-bin");
    assert!(
        is_symlink_or_junction(&private_hoist).unwrap(),
        "transitive `@pnpm.e2e/hello-world-js-bin` should be hoisted to {private_hoist:?}",
    );
    // Public-hoist patterns default to `[]` (matching pnpm v11), so
    // no transitive can match — it should NOT be at the root.
    assert!(
        !workspace.join("node_modules/@pnpm.e2e/hello-world-js-bin").exists(),
        "transitive should not be publicly hoisted under default patterns",
    );

    drop((root, mock_instance));
}

/// `publicHoistPattern: ["*"]` hoists every transitive into the
/// project's root `node_modules/`. Mirrors upstream's
/// [`hoist.ts:53` "should hoist dependencies to the root of node_modules when publicHoistPattern is used"](https://github.com/pnpm/pnpm/blob/94240bc046/installing/deps-installer/test/install/hoist.ts#L53).
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

    // Transitive should now be at the root, not under `.pnpm/`.
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

/// Both patterns empty → no hoist symlinks anywhere. Pacquet-original
/// scenario covering the issue's "Install with both patterns empty"
/// item; upstream's hoist.ts has no exact analogue but the same
/// outcome falls out of `createMatcher([])` returning a never-matches
/// matcher. The hoist pass still runs (the `is_some()` guard sees
/// `Some([])`); it just produces no entries.
#[test]
fn both_patterns_empty_produces_no_hoist_symlinks() {
    let CommandTempCwd { pacquet, pnpm, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_manifest(
        &workspace,
        serde_json::json!({ "@pnpm.e2e/hello-world-js-bin-parent": "1.0.0" }),
    );
    generate_lockfile(pnpm);
    write_workspace_yaml(&workspace, "hoistPattern: []\npublicHoistPattern: []\n");

    pacquet.with_args(["install", "--frozen-lockfile"]).assert().success();

    // Only the direct dep at the root.
    assert!(
        is_symlink_or_junction(&workspace.join("node_modules/@pnpm.e2e/hello-world-js-bin-parent"))
            .unwrap(),
    );
    assert!(
        !workspace.join("node_modules/.pnpm/node_modules/@pnpm.e2e/hello-world-js-bin").exists(),
        "no private hoist with empty patterns",
    );
    assert!(
        !workspace.join("node_modules/@pnpm.e2e/hello-world-js-bin").exists(),
        "no public hoist with empty patterns",
    );

    drop((root, mock_instance));
}

/// `shamefullyHoist: true` is the legacy alias for
/// `publicHoistPattern: ["*"]`. Mirrors upstream's translation in
/// [`extendInstallOptions`](https://github.com/pnpm/pnpm/blob/94240bc046/installing/deps-installer/src/install/extendInstallOptions.ts)
/// and pacquet's parallel handling in `WorkspaceSettings::apply_to`.
#[test]
fn shamefully_hoist_legacy_publicly_hoists_everything() {
    let CommandTempCwd { pacquet, pnpm, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_manifest(
        &workspace,
        serde_json::json!({ "@pnpm.e2e/hello-world-js-bin-parent": "1.0.0" }),
    );
    generate_lockfile(pnpm);
    write_workspace_yaml(
        &workspace,
        "shamefullyHoist: true\nhoistPattern: []\npublicHoistPattern:\n  - '*'\n",
    );

    pacquet.with_args(["install", "--frozen-lockfile"]).assert().success();

    let public_hoist = workspace.join("node_modules/@pnpm.e2e/hello-world-js-bin");
    assert!(
        is_symlink_or_junction(&public_hoist).unwrap(),
        "shamefullyHoist should publicly hoist everything",
    );

    drop((root, mock_instance));
}

/// `.modules.yaml` records `hoistedDependencies` when the hoist pass
/// runs. Mirrors the
/// [`installing/deps-restorer/test/index.ts:569` "installing with hoistPattern=*"](https://github.com/pnpm/pnpm/blob/94240bc046/installing/deps-restorer/test/index.ts)
/// assertion plus the issue's `.modules.yaml` round-trip item.
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
    // `dependencies` map. Mirrors upstream's
    // `hoistedDependencies[depPath][alias] = kind`.
    assert!(
        modules_yaml_text.contains(r#""@pnpm.e2e/hello-world-js-bin": "private""#),
        "transitive should be marked as `private` hoist; got:\n{modules_yaml_text}",
    );

    drop((root, mock_instance));
}

/// Regression for [pnpm/pnpm#11750](https://github.com/pnpm/pnpm/issues/11750):
/// pacquet's default `publicHoistPattern` must match pnpm v11's
/// (empty list) so a follow-up `pnpm` invocation in the same project
/// doesn't reject the `.modules.yaml` with
/// `ERR_PNPM_PUBLIC_HOIST_PATTERN_DIFF`. See pnpm's default at
/// <https://github.com/pnpm/pnpm/blob/1627943d2a/config/reader/src/index.ts#L184>
/// and the comparison site at
/// <https://github.com/pnpm/pnpm/blob/1627943d2a/installing/deps-installer/src/install/validateModules.ts#L67-L80>.
#[test]
fn modules_yaml_public_hoist_pattern_matches_pnpm_default() {
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
    assert!(
        modules_yaml_text.contains(r#""publicHoistPattern": []"#),
        "publicHoistPattern should serialize as an empty list (pnpm default); got:\n{modules_yaml_text}",
    );

    drop((root, mock_instance));
}

/// `hoistPattern: ["@pnpm.e2e/*"]` — only aliases under the
/// `@pnpm.e2e` scope hoist privately. Mirrors upstream's
/// [`hoist.ts:107` "should hoist dependencies by pattern"](https://github.com/pnpm/pnpm/blob/94240bc046/installing/deps-installer/test/install/hoist.ts#L107)
/// shape (different package set; the registry mock doesn't carry the
/// upstream `express` family).
#[test]
fn private_hoist_pattern_filters_aliases() {
    let CommandTempCwd { pacquet, pnpm, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_manifest(
        &workspace,
        serde_json::json!({ "@pnpm.e2e/hello-world-js-bin-parent": "1.0.0" }),
    );
    generate_lockfile(pnpm);
    write_workspace_yaml(&workspace, "hoistPattern:\n  - '@pnpm.e2e/*'\npublicHoistPattern: []\n");

    pacquet.with_args(["install", "--frozen-lockfile"]).assert().success();

    // `@pnpm.e2e/hello-world-js-bin` matches; should be private-hoisted.
    let private_hoist =
        workspace.join("node_modules/.pnpm/node_modules/@pnpm.e2e/hello-world-js-bin");
    assert!(
        is_symlink_or_junction(&private_hoist).unwrap(),
        "scoped pattern should hoist `@pnpm.e2e/hello-world-js-bin`",
    );

    drop((root, mock_instance));
}

/// `!`-negation excludes a specific alias from hoisting. Pattern
/// `["*", "!@pnpm.e2e/hello-world-js-bin"]` — everything except this
/// one alias. Mirrors the matcher's negation semantics integration-
/// test-side; the matcher itself has unit coverage in
/// `crates/config/src/matcher.rs`.
#[test]
fn negation_pattern_excludes_alias_from_hoist() {
    let CommandTempCwd { pacquet, pnpm, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_manifest(
        &workspace,
        serde_json::json!({ "@pnpm.e2e/hello-world-js-bin-parent": "1.0.0" }),
    );
    generate_lockfile(pnpm);
    write_workspace_yaml(
        &workspace,
        "hoistPattern:\n  - '*'\n  - '!@pnpm.e2e/hello-world-js-bin'\npublicHoistPattern: []\n",
    );

    pacquet.with_args(["install", "--frozen-lockfile"]).assert().success();

    let private_hoist =
        workspace.join("node_modules/.pnpm/node_modules/@pnpm.e2e/hello-world-js-bin");
    assert!(
        !private_hoist.exists(),
        "negation pattern should exclude `@pnpm.e2e/hello-world-js-bin` from hoist; \
         found at {private_hoist:?}",
    );

    drop((root, mock_instance));
}

/// Privately-hoisted bins land in `<vs>/node_modules/.bin/`. Mirrors
/// upstream's
/// [`deps-restorer/test/index.ts:569` "installing with hoistPattern=*"](https://github.com/pnpm/pnpm/blob/94240bc046/installing/deps-restorer/test/index.ts#L569)
/// assertion that `.pnpm/node_modules/.bin/hello-world-js-bin` exists.
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

    // The hoisted alias has been symlinked into `<root>/node_modules/`.
    let alias_link = workspace.join("node_modules/@pnpm.e2e/hello-world-js-bin");
    assert!(
        is_symlink_or_junction(&alias_link).unwrap(),
        "public hoist should link the alias under `<root>/node_modules/`",
    );
    // And its bin lands in `<root>/node_modules/.bin/`. Pacquet's
    // pipeline runs `SymlinkDirectDependencies` *before* the hoist
    // pass, so the install path makes a second
    // `link_direct_dep_bins` call against the public-hoisted aliases
    // after the symlinks are in place. Verify that path actually
    // produces the expected shim.
    let bin_path = workspace.join("node_modules/.bin/hello-world-js-bin");
    assert!(bin_path.exists(), "public hoist should link bin at {bin_path:?}");

    drop((root, mock_instance));
}

/// Workspace install (pnpm/pacquet#431) lands per-importer
/// `node_modules` layouts; hoist must walk every importer's direct
/// deps, not just the root, so transitives unique to a workspace
/// project still reach the shared `<vs>/node_modules` private
/// hoist. Sets up a two-importer workspace where the workspace
/// package depends on `@pnpm.e2e/hello-world-js-bin-parent` (which
/// has `@pnpm.e2e/hello-world-js-bin` as a transitive). With the
/// default `hoistPattern: ["*"]` the transitive must end up
/// hoisted regardless of which importer dragged it in.
///
/// Pacquet-original — no direct upstream analogue. Closes the
/// integration gap left by `installing/deps-installer/test/install/hoist.ts:341`
/// (which uses `mutateModulesInSingleProject` we don't have).
#[test]
fn workspace_hoist_walks_every_importer() {
    let CommandTempCwd { pacquet, pnpm, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    // Root package.json — no deps; the dependency lives only in the
    // workspace package, so the transitive can only reach the hoist
    // pass via the per-importer walk.
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({ "name": "root", "private": true }).to_string(),
    )
    .expect("write root package.json");

    // pnpm-workspace.yaml: enumerate `packages/*` (also keeps the
    // existing `storeDir`/`cacheDir` from `add_mocked_registry`).
    write_workspace_yaml(&workspace, "packages:\n  - 'packages/*'\n");

    // Workspace package — has the transitive-bearing dep.
    let pkg_dir = workspace.join("packages/foo");
    fs::create_dir_all(&pkg_dir).expect("mkdir packages/foo");
    fs::write(
        pkg_dir.join("package.json"),
        serde_json::json!({
            "name": "@local/foo",
            "version": "1.0.0",
            "private": true,
            "dependencies": { "@pnpm.e2e/hello-world-js-bin-parent": "1.0.0" },
        })
        .to_string(),
    )
    .expect("write packages/foo/package.json");

    generate_lockfile(pnpm);
    pacquet.with_args(["install", "--frozen-lockfile"]).assert().success();

    // The workspace package's direct dep symlink lives under the
    // workspace project's own `node_modules/`.
    assert!(
        is_symlink_or_junction(&pkg_dir.join("node_modules/@pnpm.e2e/hello-world-js-bin-parent"))
            .unwrap(),
        "workspace package should have its direct dep linked under its own node_modules",
    );

    // The transitive — reachable only via the workspace package —
    // must still land in the shared `<vs>/node_modules` private
    // hoist. This is what `workspace_hoist_walks_every_importer`
    // catches: a regression that filters down to just the root
    // importer would silently drop this entry.
    let private_hoist =
        workspace.join("node_modules/.pnpm/node_modules/@pnpm.e2e/hello-world-js-bin");
    assert!(
        is_symlink_or_junction(&private_hoist).unwrap(),
        "transitive of workspace package must be privately hoisted at {private_hoist:?}",
    );

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

    // Direct dep is a real directory carrying its own manifest.
    assert!(
        is_real_dir("node_modules/@pnpm.e2e/hello-world-js-bin-parent"),
        "direct dep should be a real directory under node_modules/, not a symlink",
    );
    assert!(
        workspace.join("node_modules/@pnpm.e2e/hello-world-js-bin-parent/package.json").is_file(),
        "real directory should contain the package's package.json",
    );
    // Transitive dep is hoisted flat to the top level as a real dir.
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
    // The transitive's bin is shimmed into the top-level `.bin`.
    assert!(
        workspace.join("node_modules/.bin/hello-world-js-bin").exists(),
        "hoisted linker should link the transitive's bin into node_modules/.bin",
    );
    // A fresh install writes a `pnpm-lock.yaml` for the next run.
    assert!(
        workspace.join("pnpm-lock.yaml").is_file(),
        "fresh install should write a wanted lockfile",
    );

    drop((root, mock_instance));
}

mod known_failures {
    //! Test ports of upstream `hoist.ts` cases blocked on features
    //! pacquet hasn't built yet. Each entry stubs the not-yet-built
    //! subject under test through
    //! [`pacquet_testing_utils::allow_known_failure`] so the test
    //! exits early rather than masking a real bug. The cases here
    //! cover:
    //!
    //! - **Partial install / re-hoist** ([#433]): persisted-map
    //!   preservation across re-installs, uninstall-then-rehoist,
    //!   pattern-change detection.
    //! - **`pnpm add` / `pnpm remove`**: re-running install after
    //!   adding or removing a dep requires the manifest-mutation
    //!   path pacquet doesn't expose yet.
    //! - **`--filter` selected-projects install**: pacquet doesn't
    //!   yet implement the workspace-projects-filter selection.
    //! - **`hoistWorkspacePackages`**: links workspace projects
    //!   themselves into the hoist tree (separate from snapshot
    //!   hoisting); pacquet doesn't model the
    //!   `hoistedWorkspacePackages` shape yet.
    //! - **Skipped optional deps**: hoist must not create broken
    //!   symlinks for snapshots that won't be installed; pacquet
    //!   doesn't yet skip optional deps based on OS / arch / engine.
    //! - **Direct-dep bin precedence** + **lifecycle-generated bins**:
    //!   the bin link order matters when hoisted aliases collide
    //!   with direct deps; pacquet's bin-link pipeline doesn't yet
    //!   mirror upstream's full ordering.
    //!
    //! Workspace install (pnpm/pacquet#431) landed in [#443] and is
    //! covered by [`super::workspace_hoist_walks_every_importer`].
    //!
    //! [#433]: https://github.com/pnpm/pacquet/issues/433
    //! [#443]: https://github.com/pnpm/pacquet/pull/443

    use pacquet_testing_utils::{
        allow_known_failure,
        known_failure::{KnownFailure, KnownResult},
    };

    fn partial_install_persists_hoisted_map() -> KnownResult<()> {
        Err(KnownFailure::new(
            "Partial install (pnpm/pacquet#433) is needed for re-install \
             behavior — pacquet currently does a full install on every run, \
             so `hoistedDependencies` is recomputed rather than read from \
             the existing `.modules.yaml` and merged.",
        ))
    }

    fn manifest_mutation_via_pnpm_add() -> KnownResult<()> {
        Err(KnownFailure::new(
            "Pacquet doesn't yet implement `pnpm add` / `pnpm remove` \
             manifest mutation. Upstream tests that mutate the manifest \
             between installs aren't directly portable until that lands.",
        ))
    }

    fn workspace_filter_selection() -> KnownResult<()> {
        Err(KnownFailure::new(
            "Pacquet doesn't yet implement `--filter` selected-projects \
             installs. Workspace install (pnpm/pacquet#431) landed in \
             #443 but only as the unfiltered \"install all importers\" \
             flow; selecting a subset of workspace projects is a \
             follow-up.",
        ))
    }

    fn hoist_workspace_packages_unsupported() -> KnownResult<()> {
        Err(KnownFailure::new(
            "Pacquet doesn't yet model the `hoistedWorkspacePackages` \
             shape. Workspace install lays out per-importer node_modules \
             dirs, but linking workspace projects themselves into the \
             hoist tree (the `hoistWorkspacePackages` config) requires \
             additional plumbing in `symlink_hoisted_dependencies`.",
        ))
    }

    fn skipped_optional_deps() -> KnownResult<()> {
        Err(KnownFailure::new(
            "The hoist pass already honors `SkippedSnapshots` from \
             #439's `compute_skipped_snapshots` (threaded through \
             `HoistInputs.skipped`), but the end-to-end test needs \
             an OS/arch-incompatible package in the registry mock to \
             actually exercise the skip path. Upstream's fixture \
             (`@pnpm.e2e/not-compatible-with-any-os`) isn't in \
             pacquet's mocked registry yet — a fixture-add is the \
             missing piece, not the hoist behavior.",
        ))
    }

    fn direct_dep_bin_precedence() -> KnownResult<()> {
        Err(KnownFailure::new(
            "Bin-link ordering for hoisted-vs-direct collisions \
             requires the full direct-deps bin pass plus hoist bin \
             precedence rules; pacquet's bin pipeline doesn't yet \
             implement the conflict resolution upstream uses.",
        ))
    }

    fn extend_node_path_in_shims() -> KnownResult<()> {
        Err(KnownFailure::new(
            "`extendNodePath` config (and its `false` variant) is not \
             read or applied to command shims by pacquet yet.",
        ))
    }

    fn external_symlink_introspection() -> KnownResult<()> {
        Err(KnownFailure::new(
            "Pacquet's `symlink_package` swallows EEXIST silently — the \
             conservative path. Upstream additionally introspects the \
             existing entry via `resolveLinkTarget` + `isSubdir` to \
             distinguish a real directory (preserve), an external \
             symlink (preserve), and a virtual-store-resident symlink \
             (overwrite). That introspection isn't ported yet.",
        ))
    }

    /// Upstream: [`hoist.ts:24` "should hoist dependencies"](https://github.com/pnpm/pnpm/blob/94240bc046/installing/deps-installer/test/install/hoist.ts#L24).
    /// Repeats the install both as non-headless and as
    /// `frozenLockfile: true` to assert `hoistedDependencies` is
    /// preserved verbatim. Pacquet recomputes the map on every
    /// install (no partial-install path yet), so byte-for-byte
    /// preservation isn't testable here.
    #[test]
    fn should_hoist_dependencies_repeat_install_preserves_map() {
        allow_known_failure!(partial_install_persists_hoisted_map());
    }

    /// Upstream: [`hoist.ts:71` "public hoist should not override directories that are already in the root of node_modules"](https://github.com/pnpm/pnpm/blob/94240bc046/installing/deps-installer/test/install/hoist.ts#L71).
    /// Tests that pre-existing real directories at the public hoist
    /// target are preserved. Pacquet's `symlink_package` returns Ok
    /// on `EEXIST` — the conservative path — but the upstream test
    /// also covers the "external symlink" case which requires the
    /// `resolveLinkTarget` + `isSubdir` introspection pacquet doesn't
    /// yet do.
    #[test]
    fn public_hoist_preserves_existing_root_directories() {
        allow_known_failure!(external_symlink_introspection());
    }

    /// Upstream: [`hoist.ts:121` "should remove hoisted dependencies"](https://github.com/pnpm/pnpm/blob/94240bc046/installing/deps-installer/test/install/hoist.ts#L121).
    /// Removes a dependency and asserts the hoist symlinks for its
    /// transitives go too. Needs the partial-install / pruning path.
    #[test]
    fn should_remove_hoisted_dependencies() {
        allow_known_failure!(partial_install_persists_hoisted_map());
    }

    /// Upstream: [`hoist.ts:137` "should not override root packages with hoisted dependencies"](https://github.com/pnpm/pnpm/blob/94240bc046/installing/deps-installer/test/install/hoist.ts#L137).
    #[test]
    fn should_not_override_root_packages_with_hoisted_deps() {
        allow_known_failure!(partial_install_persists_hoisted_map());
    }

    /// Upstream: [`hoist.ts:148` "should rehoist when uninstalling a package"](https://github.com/pnpm/pnpm/blob/94240bc046/installing/deps-installer/test/install/hoist.ts#L148).
    #[test]
    fn should_rehoist_when_uninstalling_a_package() {
        allow_known_failure!(partial_install_persists_hoisted_map());
    }

    /// Upstream: [`hoist.ts:169` "should rehoist after running a general install"](https://github.com/pnpm/pnpm/blob/94240bc046/installing/deps-installer/test/install/hoist.ts#L169).
    #[test]
    fn should_rehoist_after_running_a_general_install() {
        allow_known_failure!(partial_install_persists_hoisted_map());
    }

    /// Upstream: [`hoist.ts:201` "should not override aliased dependencies"](https://github.com/pnpm/pnpm/blob/94240bc046/installing/deps-installer/test/install/hoist.ts#L201).
    /// npm-aliases — the alias `foo` resolves to `bar@x` in the
    /// importer, and a transitive `foo` shouldn't override that.
    /// Pacquet's algo handles this via the `currentSpecifiers` seed,
    /// but verifying end-to-end needs alias-aware lockfile handling
    /// throughout the install pipeline that isn't fully wired yet.
    #[test]
    fn should_not_override_aliased_dependencies() {
        allow_known_failure!(partial_install_persists_hoisted_map());
    }

    /// Upstream: [`hoist.ts:209` "hoistPattern=* throws exception when executed on node_modules installed w/o the option"](https://github.com/pnpm/pnpm/blob/94240bc046/installing/deps-installer/test/install/hoist.ts#L209).
    /// Pattern-change detection between `.modules.yaml` and current
    /// config triggers `MODULES_BREAKING_CHANGE` upstream. Pacquet
    /// doesn't yet read the persisted patterns and compare, so
    /// pattern-change detection is a follow-up.
    #[test]
    fn hoist_pattern_mismatch_throws_against_existing_modules_yaml() {
        allow_known_failure!(partial_install_persists_hoisted_map());
    }

    /// Upstream: [`hoist.ts:220` "hoistPattern=undefined throws exception when executed on node_modules installed with hoist-pattern=*"](https://github.com/pnpm/pnpm/blob/94240bc046/installing/deps-installer/test/install/hoist.ts#L220).
    #[test]
    fn hoist_pattern_undefined_throws_against_hoisted_modules_yaml() {
        allow_known_failure!(partial_install_persists_hoisted_map());
    }

    /// Upstream: [`hoist.ts:233` "hoist by alias"](https://github.com/pnpm/pnpm/blob/94240bc046/installing/deps-installer/test/install/hoist.ts#L233).
    /// Hoisting respects npm-aliased package names — the alias is
    /// the directory name, not the package name. Pacquet's algo
    /// uses the alias correctly (the test in `hoist/tests.rs`
    /// covers it at the unit level) but the end-to-end integration
    /// requires alias-aware lockfile + manifest data not all wired.
    #[test]
    fn hoist_by_alias() {
        allow_known_failure!(partial_install_persists_hoisted_map());
    }

    /// Upstream: [`hoist.ts:249` "should remove aliased hoisted dependencies"](https://github.com/pnpm/pnpm/blob/94240bc046/installing/deps-installer/test/install/hoist.ts#L249).
    #[test]
    fn should_remove_aliased_hoisted_dependencies() {
        allow_known_failure!(partial_install_persists_hoisted_map());
    }

    /// Upstream: [`hoist.ts:272` "should update .modules.yaml when pruning if we are flattening"](https://github.com/pnpm/pnpm/blob/94240bc046/installing/deps-installer/test/install/hoist.ts#L272).
    #[test]
    fn modules_yaml_updated_on_prune_when_flattening() {
        allow_known_failure!(partial_install_persists_hoisted_map());
    }

    /// Upstream: [`hoist.ts:288` "should rehoist after pruning"](https://github.com/pnpm/pnpm/blob/94240bc046/installing/deps-installer/test/install/hoist.ts#L288).
    #[test]
    fn should_rehoist_after_pruning() {
        allow_known_failure!(partial_install_persists_hoisted_map());
    }

    /// Upstream: [`hoist.ts:320` "should hoist correctly peer dependencies"](https://github.com/pnpm/pnpm/blob/94240bc046/installing/deps-installer/test/install/hoist.ts#L320).
    /// Peer deps split into multiple snapshot keys (one per
    /// peer-resolution variant). Hoist must pick the right variant
    /// per importer. Pacquet's lockfile parser handles peers but
    /// the install path doesn't yet exercise the multi-variant case
    /// the upstream test depends on.
    #[test]
    fn should_hoist_correctly_peer_dependencies() {
        allow_known_failure!(partial_install_persists_hoisted_map());
    }

    /// Upstream: [`hoist.ts:327` "should uninstall correctly peer dependencies"](https://github.com/pnpm/pnpm/blob/94240bc046/installing/deps-installer/test/install/hoist.ts#L327).
    #[test]
    fn should_uninstall_correctly_peer_dependencies() {
        allow_known_failure!(partial_install_persists_hoisted_map());
    }

    /// Upstream: [`hoist.ts:341` "hoist-pattern: hoist all dependencies to the virtual store node_modules"](https://github.com/pnpm/pnpm/blob/94240bc046/installing/deps-installer/test/install/hoist.ts#L341).
    /// Workspace install followed by frozen reinstall. Pacquet's
    /// per-importer hoist walk lands the basic shape — covered by
    /// [`super::workspace_hoist_walks_every_importer`] — but the
    /// upstream test additionally re-installs and asserts
    /// preservation, which needs partial install ([#433]).
    ///
    /// [#433]: https://github.com/pnpm/pacquet/issues/433
    #[test]
    fn workspace_hoist_all_to_virtual_store_node_modules() {
        allow_known_failure!(partial_install_persists_hoisted_map());
    }

    /// Upstream: [`hoist.ts:423` "hoist when updating in one of the workspace projects"](https://github.com/pnpm/pnpm/blob/94240bc046/installing/deps-installer/test/install/hoist.ts#L423).
    /// Mutates the workspace package's `package.json` mid-test and
    /// re-installs — needs `pnpm add`-equivalent manifest mutation.
    #[test]
    fn workspace_hoist_when_updating_one_project() {
        allow_known_failure!(manifest_mutation_via_pnpm_add());
    }

    /// Upstream: [`hoist.ts:514` "should recreate node_modules with hoisting"](https://github.com/pnpm/pnpm/blob/94240bc046/installing/deps-installer/test/install/hoist.ts#L514).
    /// Removes `node_modules` and re-installs from the lockfile —
    /// needs partial-install state for the rehoist comparison.
    #[test]
    fn should_recreate_node_modules_with_hoisting() {
        allow_known_failure!(partial_install_persists_hoisted_map());
    }

    /// Upstream: [`hoist.ts:540` "hoisting should not create a broken symlink to a skipped optional dependency"](https://github.com/pnpm/pnpm/blob/94240bc046/installing/deps-installer/test/install/hoist.ts#L540).
    #[test]
    fn hoisting_skips_broken_symlink_for_skipped_optional() {
        allow_known_failure!(skipped_optional_deps());
    }

    /// Upstream: [`hoist.ts:567` "the hoisted packages should not override the bin files of the direct dependencies"](https://github.com/pnpm/pnpm/blob/94240bc046/installing/deps-installer/test/install/hoist.ts#L567).
    #[test]
    fn hoisted_packages_dont_override_direct_dep_bins() {
        allow_known_failure!(direct_dep_bin_precedence());
    }

    /// Upstream: [`hoist.ts:587` "hoist packages which is in the dependencies tree of the selected projects"](https://github.com/pnpm/pnpm/blob/94240bc046/installing/deps-installer/test/install/hoist.ts#L587).
    /// Uses upstream's `selectedProjectDirs` API to install a
    /// subset of workspace projects. Pacquet doesn't yet implement
    /// `--filter` selected-projects installs.
    #[test]
    fn workspace_hoist_packages_in_selected_projects_tree() {
        allow_known_failure!(workspace_filter_selection());
    }

    /// Upstream: [`hoist.ts:682` "only hoist packages which is in the dependencies tree of the selected projects with sub dependencies"](https://github.com/pnpm/pnpm/blob/94240bc046/installing/deps-installer/test/install/hoist.ts#L682).
    /// Same `selectedProjectDirs` shape as above.
    #[test]
    fn workspace_hoist_only_in_selected_projects_with_subdeps() {
        allow_known_failure!(workspace_filter_selection());
    }

    /// Upstream: [`hoist.ts:790` "should add extra node paths to command shims"](https://github.com/pnpm/pnpm/blob/94240bc046/installing/deps-installer/test/install/hoist.ts#L790).
    #[test]
    fn should_add_extra_node_paths_to_command_shims() {
        allow_known_failure!(extend_node_path_in_shims());
    }

    /// Upstream: [`hoist.ts:799` "should not add extra node paths to command shims, when extend-node-path is set to false"](https://github.com/pnpm/pnpm/blob/94240bc046/installing/deps-installer/test/install/hoist.ts#L799).
    #[test]
    fn should_not_add_extra_node_paths_when_extend_node_path_false() {
        allow_known_failure!(extend_node_path_in_shims());
    }

    /// Upstream: [`hoist.ts:813` "hoistWorkspacePackages should hoist all workspace projects"](https://github.com/pnpm/pnpm/blob/94240bc046/installing/deps-installer/test/install/hoist.ts#L813).
    /// Tests the `hoistWorkspacePackages: true` config which links
    /// workspace projects themselves into the hoist tree (separate
    /// from snapshot hoisting). Needs the
    /// `hoistedWorkspacePackages` shape pacquet doesn't model yet.
    #[test]
    fn hoist_workspace_packages_hoists_all_workspace_projects() {
        allow_known_failure!(hoist_workspace_packages_unsupported());
    }

    /// Upstream: [`hoist.ts:89` "should hoist some dependencies to the root of node_modules when publicHoistPattern is used and others to the virtual store directory"](https://github.com/pnpm/pnpm/blob/94240bc046/installing/deps-installer/test/install/hoist.ts#L89).
    /// Combined-pattern shape — public for some, private for others.
    /// Pacquet's algo handles this (the unit test
    /// `public_pattern_wins_ties` covers the precedence) but the
    /// upstream end-to-end test uses `express` + the eslint family,
    /// which the registry mock doesn't carry.
    #[test]
    fn combined_public_and_private_hoist_patterns_split_targets() {
        allow_known_failure!(partial_install_persists_hoisted_map());
    }
}
