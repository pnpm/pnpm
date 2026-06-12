//! End-to-end coverage for `nodeLinker: hoisted` on the
//! **fresh-lockfile** install path (no checked-in lockfile, not
//! `--frozen-lockfile`). pnpm/pnpm#11871 enabled this path; before
//! it, `pacquet install` hard-refused the combination.
//!
//! Each test writes a `package.json` (and a `pnpm-workspace.yaml`
//! carrying `nodeLinker: hoisted` plus any feature knob under test),
//! then runs `pacquet install` so the fresh resolver builds the
//! lockfile in memory and the hoisted linker materializes a flat
//! `node_modules/` of **real directories**.
//!
//! Test ports of upstream's
//! [`installing/deps-installer/test/hoistedNodeLinker/install.ts`](https://github.com/pnpm/pnpm/blob/94240bc046/installing/deps-installer/test/hoistedNodeLinker/install.ts).
//! Cases that depend on features pacquet hasn't built yet — `pnpm add`
//! / update manifest mutation (pnpm/pacquet#433), lifecycle scripts +
//! bin linking on the fresh path ([#11870]) — live in [`known_failures`]
//! below with [`pacquet_testing_utils::allow_known_failure`] gating the
//! assertion against the not-yet-implemented subject under test.
//!
//! [#11870]: https://github.com/pnpm/pnpm/issues/11870

#![cfg(unix)] // hoisted bin shims + real-dir-vs-junction checks are unix-shaped here.

pub mod _utils;
pub use _utils::*;

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pacquet_testing_utils::{
    bin::{AddMockedRegistry, CommandTempCwd},
    fs::is_symlink_or_junction,
};
use std::{fs, path::Path};

/// Replace the `pnpm-workspace.yaml` written by `add_mocked_registry`
/// with one that keeps the mock's `storeDir` / `cacheDir` and appends
/// `extra` (e.g. `nodeLinker: hoisted`).
fn write_workspace_yaml(workspace: &Path, extra: &str) {
    let yaml = format!("storeDir: ../pacquet-store\ncacheDir: ../pacquet-cache\n{extra}");
    fs::write(workspace.join("pnpm-workspace.yaml"), yaml).expect("write pnpm-workspace.yaml");
}

/// Write a `package.json` with the given `dependencies` object.
#[expect(
    clippy::needless_pass_by_value,
    reason = "test helper called many times with json!(...) literals; owned arg keeps call sites clean"
)]
fn write_manifest(workspace: &Path, deps: serde_json::Value) {
    let manifest = serde_json::json!({ "dependencies": deps });
    fs::write(workspace.join("package.json"), manifest.to_string()).expect("write package.json");
}

/// `true` when `relative` resolves to a real directory (not a symlink
/// or junction) under `workspace`. This is the hoisted-linker
/// contract: regular deps are materialized as real directories, not
/// symlinks into a virtual store. Mirrors upstream's
/// `realpathSync(p) === resolve(p)` check.
fn is_real_dir(workspace: &Path, relative: &str) -> bool {
    let path = workspace.join(relative);
    path.is_dir() && !is_symlink_or_junction(&path).unwrap()
}

/// Upstream: [`install.ts:16` "installing with hoisted node-linker"](https://github.com/pnpm/pnpm/blob/94240bc046/installing/deps-installer/test/hoistedNodeLinker/install.ts#L16).
///
/// Direct deps land as real directories at the project root and a
/// version-conflicting transitive nests under its consumer. `send`
/// pulls `ms@2.x` while the root pins `ms@1.0.0`, so the root keeps
/// `1.0.0` and `send` nests its own `ms`. `.modules.yaml` records the
/// hoisted linker.
///
/// The upstream test also removes `node_modules/send` and reinstalls
/// to assert it is re-added; that re-add is the partial-install path
/// (pnpm/pacquet#433) and is omitted here.
#[test]
fn installing_with_hoisted_node_linker() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_manifest(
        &workspace,
        serde_json::json!({ "send": "0.17.2", "has-flag": "1.0.0", "ms": "1.0.0" }),
    );
    write_workspace_yaml(&workspace, "nodeLinker: hoisted\n");

    pacquet.with_args(["install"]).assert().success();

    assert!(is_real_dir(&workspace, "node_modules/send"), "send should be a real directory");
    assert!(
        is_real_dir(&workspace, "node_modules/has-flag"),
        "has-flag should be a real directory",
    );
    assert!(is_real_dir(&workspace, "node_modules/ms"), "ms should be a real directory");
    // Version conflict: send needs ms@2.x, the root pins ms@1.0.0, so
    // send keeps its own copy nested.
    assert!(
        workspace.join("node_modules/send/node_modules/ms").exists(),
        "send's conflicting ms should nest under send/node_modules/ms",
    );

    // `.modules.yaml` is written JSON-with-quoted-keys (valid YAML);
    // a substring match avoids dragging in a YAML parser, matching the
    // convention in the sibling `hoist.rs` tests.
    let modules_yaml = fs::read_to_string(workspace.join("node_modules/.modules.yaml"))
        .expect("read .modules.yaml");
    assert!(
        modules_yaml.contains(r#""nodeLinker": "hoisted""#),
        ".modules.yaml should record the hoisted linker; got:\n{modules_yaml}",
    );

    drop((root, mock_instance));
}

/// Upstream: [`install.ts:45` "installing with hoisted node-linker and no lockfile"](https://github.com/pnpm/pnpm/blob/94240bc046/installing/deps-installer/test/hoistedNodeLinker/install.ts#L45).
///
/// With `lockfile: false` the hoisted install still materializes a
/// real directory and writes no `pnpm-lock.yaml`.
#[test]
fn installing_with_hoisted_node_linker_and_no_lockfile() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_manifest(&workspace, serde_json::json!({ "ms": "1.0.0" }));
    write_workspace_yaml(&workspace, "nodeLinker: hoisted\nlockfile: false\n");

    pacquet.with_args(["install"]).assert().success();

    assert!(is_real_dir(&workspace, "node_modules/ms"), "ms should be a real directory");
    assert!(
        !workspace.join("pnpm-lock.yaml").exists(),
        "no lockfile should be written when lockfile: false",
    );

    drop((root, mock_instance));
}

/// Upstream: [`install.ts:229` "hoistingLimits should prevent packages to be hoisted"](https://github.com/pnpm/pnpm/blob/89812a9353/installing/deps-installer/test/hoistedNodeLinker/install.ts#L229).
///
/// `hoistingLimits: dependencies` borders each direct dependency, so
/// `send`'s transitive `ms` stays nested under `send` instead of
/// hoisting to the root.
#[test]
fn hoisting_limits_prevents_hoisting() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_manifest(&workspace, serde_json::json!({ "send": "0.17.2" }));
    write_workspace_yaml(&workspace, "nodeLinker: hoisted\nhoistingLimits: dependencies\n");

    pacquet.with_args(["install"]).assert().success();

    assert!(
        !workspace.join("node_modules/ms").exists(),
        "ms should not be hoisted to the root when send's deps are bordered",
    );
    assert!(
        workspace.join("node_modules/send/node_modules/ms").exists(),
        "ms should stay nested under send",
    );

    drop((root, mock_instance));
}

/// Upstream: [`install.ts:247` "externalDependencies should prevent package from being hoisted to the root"](https://github.com/pnpm/pnpm/blob/94240bc046/installing/deps-installer/test/hoistedNodeLinker/install.ts#L247).
///
/// `externalDependencies: [ms]` reserves the root `ms` slot for an
/// external linker, so `ms` is not hoisted to the root and stays
/// nested under `send`.
#[test]
fn external_dependencies_prevents_hoisting_to_root() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_manifest(&workspace, serde_json::json!({ "send": "0.17.2" }));
    write_workspace_yaml(&workspace, "nodeLinker: hoisted\nexternalDependencies:\n  - ms\n");

    pacquet.with_args(["install"]).assert().success();

    assert!(
        !workspace.join("node_modules/ms").exists(),
        "ms should not be hoisted to the root when declared external",
    );
    assert!(
        workspace.join("node_modules/send/node_modules/ms").exists(),
        "ms should stay nested under send",
    );

    drop((root, mock_instance));
}

/// Upstream: [`install.ts:314` "peerDependencies should be installed when autoInstallPeers is set to true and nodeLinker is set to hoisted"](https://github.com/pnpm/pnpm/blob/94240bc046/installing/deps-installer/test/hoistedNodeLinker/install.ts#L314).
///
/// With `autoInstallPeers: true`, `react-dom`'s `react` peer is
/// resolved and lands as a real directory at the hoisted root.
#[test]
fn peer_dependencies_installed_with_auto_install_peers() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_manifest(&workspace, serde_json::json!({ "react-dom": "18.2.0" }));
    write_workspace_yaml(&workspace, "nodeLinker: hoisted\nautoInstallPeers: true\n");

    pacquet.with_args(["install"]).assert().success();

    assert!(
        workspace.join("node_modules/react").exists(),
        "react peer should be installed under the hoisted root",
    );

    drop((root, mock_instance));
}

mod known_failures {
    //! Ports of upstream `hoistedNodeLinker/install.ts` cases blocked
    //! on features pacquet hasn't built yet. Each stubs the
    //! not-yet-built subject through
    //! [`pacquet_testing_utils::allow_known_failure`] so the test exits
    //! early rather than masking a real bug.

    use pacquet_testing_utils::{
        allow_known_failure,
        known_failure::{KnownFailure, KnownResult},
    };

    fn manifest_mutation_via_pnpm_add() -> KnownResult<()> {
        Err(KnownFailure::new(
            "Pacquet doesn't yet implement the `pnpm add` / update \
             manifest-mutation flow these tests exercise (add a dep, or \
             bump a dist-tag, then reinstall). Partial install / re-hoist \
             across runs is tracked by pnpm/pacquet#433.",
        ))
    }

    fn lifecycle_scripts_on_fresh_path() -> KnownResult<()> {
        Err(KnownFailure::new(
            "The fresh-lockfile install path doesn't run lifecycle \
             scripts or link per-node_modules bins yet — that's the \
             `BuildModules` port tracked by #11870. Until it lands, \
             pre/postinstall-script and local-bin assertions under the \
             hoisted linker can't be exercised on the fresh path.",
        ))
    }

    /// Upstream: [`install.ts:61` "overwriting (is-positive@3.0.0 with is-positive@latest)"](https://github.com/pnpm/pnpm/blob/94240bc046/installing/deps-installer/test/hoistedNodeLinker/install.ts#L61).
    #[test]
    fn overwriting_is_positive_with_latest() {
        allow_known_failure!(manifest_mutation_via_pnpm_add());
    }

    /// Upstream: [`install.ts:83` "overwriting existing files in node_modules"](https://github.com/pnpm/pnpm/blob/94240bc046/installing/deps-installer/test/hoistedNodeLinker/install.ts#L83).
    #[test]
    fn overwriting_existing_files_in_node_modules() {
        allow_known_failure!(manifest_mutation_via_pnpm_add());
    }

    /// Upstream: [`install.ts:97` "preserve subdeps on update"](https://github.com/pnpm/pnpm/blob/94240bc046/installing/deps-installer/test/hoistedNodeLinker/install.ts#L97).
    #[test]
    fn preserve_subdeps_on_update() {
        allow_known_failure!(manifest_mutation_via_pnpm_add());
    }

    /// Upstream: [`install.ts:119` "adding a new dependency to one of the workspace projects"](https://github.com/pnpm/pnpm/blob/94240bc046/installing/deps-installer/test/hoistedNodeLinker/install.ts#L119).
    #[test]
    fn adding_a_new_dependency_to_a_workspace_project() {
        allow_known_failure!(manifest_mutation_via_pnpm_add());
    }

    /// Upstream: [`install.ts:172` "installing the same package with alias and no alias"](https://github.com/pnpm/pnpm/blob/94240bc046/installing/deps-installer/test/hoistedNodeLinker/install.ts#L172).
    /// Relies on `pnpm add` of multiple specifiers plus a dist-tag
    /// bump to pin the aliased and unaliased copies to the same
    /// version.
    #[test]
    fn installing_same_package_with_alias_and_no_alias() {
        allow_known_failure!(manifest_mutation_via_pnpm_add());
    }

    /// Upstream: [`install.ts:329` "installing with hoisted node-linker a package that is a peer dependency of itself"](https://github.com/pnpm/pnpm/blob/94240bc046/installing/deps-installer/test/hoistedNodeLinker/install.ts#L329).
    /// Adds the dep via `pnpm add --save` and then introspects the
    /// written lockfile's `peerDependencies` entry.
    #[test]
    fn package_that_is_peer_dependency_of_itself() {
        allow_known_failure!(manifest_mutation_via_pnpm_add());
    }

    /// Upstream: [`install.ts:187` "run pre/postinstall scripts. bin files should be linked in a hoisted node_modules"](https://github.com/pnpm/pnpm/blob/94240bc046/installing/deps-installer/test/hoistedNodeLinker/install.ts#L187).
    #[test]
    fn run_pre_and_postinstall_scripts_and_link_bins() {
        allow_known_failure!(lifecycle_scripts_on_fresh_path());
    }

    /// Upstream: [`install.ts:210` "running install scripts in a workspace that has no root project"](https://github.com/pnpm/pnpm/blob/94240bc046/installing/deps-installer/test/hoistedNodeLinker/install.ts#L210).
    #[test]
    fn running_install_scripts_in_workspace_without_root_project() {
        allow_known_failure!(lifecycle_scripts_on_fresh_path());
    }

    /// Upstream: [`install.ts:264` "linking bins of local projects when node-linker is set to hoisted"](https://github.com/pnpm/pnpm/blob/94240bc046/installing/deps-installer/test/hoistedNodeLinker/install.ts#L264).
    #[test]
    fn linking_bins_of_local_projects() {
        allow_known_failure!(lifecycle_scripts_on_fresh_path());
    }
}
