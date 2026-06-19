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
use std::{fs, path::Path, process::Command};

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

/// Build a fresh `pacquet` `Command` rooted at `workspace`. Needed to
/// drive a second invocation in the same workspace because
/// [`assert_cmd::Command::assert`] consumes the wrapped command. The
/// mock registry is configured through the workspace's `.npmrc` /
/// `pnpm-workspace.yaml`, so a command that merely runs in `workspace`
/// inherits it without extra env.
fn pacquet_at(workspace: &Path) -> Command {
    Command::cargo_bin("pacquet").expect("find the pacquet binary").with_current_dir(workspace)
}

/// `rm -rf` that tolerates an already-absent path.
fn fs_remove_dir_all(path: &Path) {
    match fs::remove_dir_all(path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => panic!("remove {path:?}: {error}"),
    }
}

/// Read the `version` field of the `package.json` at
/// `workspace/relative`. Used by the workspace tests to tell which
/// version of a conflicting dependency landed at each location.
fn read_pkg_version(workspace: &Path, relative: &str) -> String {
    let manifest = fs::read_to_string(workspace.join(relative).join("package.json"))
        .unwrap_or_else(|error| panic!("read {relative}/package.json: {error}"));
    let parsed: serde_json::Value =
        serde_json::from_str(&manifest).expect("parse package.json as JSON");
    parsed["version"].as_str().expect("package.json has a string version").to_string()
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

/// Upstream: [`installing/deps-restorer/test/index.ts:859` "installing with node-linker=hoisted"](https://github.com/pnpm/pnpm/blob/94240bc046/installing/deps-restorer/test/index.ts#L859).
///
/// The headless (frozen-lockfile) path materializes the hoisted
/// layout from a pre-existing lockfile, reproducing the same
/// real-dir + version-conflict-nesting shape as a fresh install.
#[test]
fn installing_with_hoisted_node_linker_frozen() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_manifest(
        &workspace,
        serde_json::json!({ "send": "0.17.2", "has-flag": "1.0.0", "ms": "1.0.0" }),
    );
    write_workspace_yaml(&workspace, "nodeLinker: hoisted\n");

    // Seed the lockfile and node_modules.
    pacquet.with_args(["install"]).assert().success();
    assert!(workspace.join("pnpm-lock.yaml").exists(), "first install writes the lockfile");

    // Tear down node_modules so the frozen install is a pure replay.
    fs_remove_dir_all(&workspace.join("node_modules"));

    pacquet_at(&workspace).with_arg("install").with_arg("--frozen-lockfile").assert().success();

    assert!(is_real_dir(&workspace, "node_modules/send"), "send is a real dir after frozen replay");
    assert!(is_real_dir(&workspace, "node_modules/ms"), "ms is a real dir after frozen replay");
    assert!(
        workspace.join("node_modules/send/node_modules/ms").exists(),
        "send's conflicting ms nests under send after frozen replay",
    );

    let modules_yaml = fs::read_to_string(workspace.join("node_modules/.modules.yaml"))
        .expect("read .modules.yaml");
    assert!(
        modules_yaml.contains(r#""nodeLinker": "hoisted""#),
        ".modules.yaml records the hoisted linker; got:\n{modules_yaml}",
    );

    drop((root, mock_instance));
}

/// Upstream: [`installing/deps-restorer/test/index.ts:873` "installing in a workspace with node-linker=hoisted"](https://github.com/pnpm/pnpm/blob/94240bc046/installing/deps-restorer/test/index.ts#L873).
///
/// Workspace-wide hoisting under the frozen path. When the root
/// importer and a workspace project pin conflicting versions of one
/// name, the root's version wins the top-level slot — root deps rank
/// first in the hoister's preference order — and the project's
/// version nests under its own `node_modules`. Mirrors the upstream
/// layout where the root's `webpack@5.65.0` lands at the root and
/// `foo`'s `webpack@2.7.0` nests under `foo`.
#[test]
fn installing_in_a_workspace_with_hoisted_node_linker_frozen() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "name": "ws-root",
            "version": "0.0.0",
            "private": true,
            "dependencies": { "ms": "2.1.3" },
        })
        .to_string(),
    )
    .expect("write root package.json");

    write_workspace_yaml(&workspace, "nodeLinker: hoisted\npackages:\n  - 'packages/*'\n");

    fs::create_dir_all(workspace.join("packages/foo")).expect("mkdir packages/foo");
    fs::write(
        workspace.join("packages/foo/package.json"),
        serde_json::json!({
            "name": "foo",
            "version": "1.0.0",
            "dependencies": { "ms": "2.0.0" },
        })
        .to_string(),
    )
    .expect("write packages/foo/package.json");

    // Seed the lockfile, then replay frozen.
    pacquet.with_args(["install"]).assert().success();
    fs_remove_dir_all(&workspace.join("node_modules"));
    fs_remove_dir_all(&workspace.join("packages/foo/node_modules"));

    pacquet_at(&workspace).with_arg("install").with_arg("--frozen-lockfile").assert().success();

    assert!(is_real_dir(&workspace, "node_modules/ms"), "root ms is a real dir");
    assert_eq!(
        read_pkg_version(&workspace, "node_modules/ms"),
        "2.1.3",
        "the root importer's ms@2.1.3 wins the top-level slot",
    );
    assert!(
        is_real_dir(&workspace, "packages/foo/node_modules/ms"),
        "foo's conflicting ms is a real dir nested under foo",
    );
    assert_eq!(
        read_pkg_version(&workspace, "packages/foo/node_modules/ms"),
        "2.0.0",
        "foo's conflicting ms@2.0.0 nests under the project",
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

#[test]
fn package_map_resolves_declared_hoisted_dependencies_at_runtime() {
    if node_major() < 27 {
        eprintln!("skipping package-map runtime smoke: Node.js major is below 27");
        return;
    }
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_manifest(&workspace, serde_json::json!({ "@pnpm.e2e/pkg-with-1-dep": "100.0.0" }));
    write_workspace_yaml(&workspace, "nodeLinker: hoisted\n");

    pacquet.with_args(["install"]).assert().success();

    let root_dependency_dir = root_dependency_dir(&workspace, "@pnpm.e2e/pkg-with-1-dep");
    let smoke = root_dependency_dir.join("package-map-smoke.cjs");
    fs::write(&smoke, "require('@pnpm.e2e/dep-of-pkg-with-1-dep')\n").expect("write smoke file");
    let output = run_node_with_package_map(&workspace, &smoke);
    assert!(
        output.status.success(),
        "declared package should resolve with package map\nstdout:\n{}\nstderr:\n{}\npackage map:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
        package_map_contents(&workspace),
    );

    drop((root, mock_instance));
}

#[test]
fn standard_package_map_blocks_undeclared_hoisted_dependencies_at_runtime() {
    if node_major() < 27 {
        eprintln!("skipping package-map runtime smoke: Node.js major is below 27");
        return;
    }
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_manifest(
        &workspace,
        serde_json::json!({
            "@pnpm.e2e/foo": "100.0.0",
            "@pnpm.e2e/pkg-with-1-dep": "100.0.0",
        }),
    );
    write_workspace_yaml(&workspace, "nodeLinker: hoisted\n");

    pacquet.with_args(["install"]).assert().success();

    let root_dependency_dir = root_dependency_dir(&workspace, "@pnpm.e2e/pkg-with-1-dep");
    let smoke = root_dependency_dir.join("package-map-block-smoke.cjs");
    fs::write(&smoke, "require('@pnpm.e2e/foo/package.json')\n").expect("write smoke file");
    let output = run_node_with_package_map(&workspace, &smoke);
    assert!(
        !output.status.success(),
        "undeclared hoisted package should not resolve in standard package-map mode",
    );

    drop((root, mock_instance));
}

#[test]
fn loose_package_map_allows_undeclared_hoisted_dependencies_at_runtime() {
    if node_major() < 27 {
        eprintln!("skipping package-map runtime smoke: Node.js major is below 27");
        return;
    }
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_manifest(
        &workspace,
        serde_json::json!({
            "@pnpm.e2e/foo": "100.0.0",
            "@pnpm.e2e/pkg-with-1-dep": "100.0.0",
        }),
    );
    write_workspace_yaml(&workspace, "nodeLinker: hoisted\nnodePackageMapType: loose\n");

    pacquet.with_args(["install"]).assert().success();

    let root_dependency_dir = root_dependency_dir(&workspace, "@pnpm.e2e/pkg-with-1-dep");
    let smoke = root_dependency_dir.join("package-map-loose-smoke.cjs");
    fs::write(&smoke, "require('@pnpm.e2e/foo/package.json')\n").expect("write smoke file");
    let output = run_node_with_package_map(&workspace, &smoke);
    assert!(
        output.status.success(),
        "undeclared hoisted package should resolve in loose package-map mode\nstdout:\n{}\nstderr:\n{}\npackage map:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
        package_map_contents(&workspace),
    );

    drop((root, mock_instance));
}

fn run_node_with_package_map(workspace: &Path, script: &Path) -> std::process::Output {
    Command::new("node")
        .arg(format!(
            "--experimental-package-map={}",
            workspace.join("node_modules/.package-map.json").display(),
        ))
        .arg(script)
        .current_dir(workspace)
        .output()
        .expect("run Node.js")
}

fn package_map_contents(workspace: &Path) -> String {
    fs::read_to_string(workspace.join("node_modules/.package-map.json"))
        .unwrap_or_else(|error| format!("failed to read package map: {error}"))
}

fn root_dependency_dir(workspace: &Path, name: &str) -> std::path::PathBuf {
    let package_map: serde_json::Value =
        serde_json::from_str(&package_map_contents(workspace)).expect("parse package map");
    let dependency_id =
        package_map["packages"]["."]["dependencies"][name].as_str().expect("root dependency id");
    let url = package_map["packages"][dependency_id]["url"].as_str().expect("dependency url");
    workspace.join("node_modules").join(url)
}

fn node_major() -> u32 {
    let output = Command::new("node").arg("--version").output().expect("run node --version");
    assert!(output.status.success(), "node --version should succeed");
    let version = String::from_utf8(output.stdout).expect("node version is utf8");
    version
        .trim()
        .strip_prefix('v')
        .unwrap_or_else(|| version.trim())
        .split('.')
        .next()
        .expect("node version has a major")
        .parse()
        .expect("node major is numeric")
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
