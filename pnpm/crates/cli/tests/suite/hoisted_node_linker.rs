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

#![cfg(unix)] // hoisted bin shims + real-dir-vs-junction checks are unix-shaped here.

use crate::_utils;
pub use _utils::*;

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::{
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
/// symlinks into a virtual store.
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
    Command::cargo_bin("pnpm").expect("find the pnpm binary").with_current_dir(workspace)
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

/// The `added` counter on the progress line is fed by
/// `pnpm:progress imported`, which under `nodeLinker: hoisted` only
/// the hoisted linker emits — an install that materializes packages
/// must move it off zero (pnpm/pnpm#14348).
#[test]
fn the_progress_line_counts_the_packages_the_hoisted_linker_added() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_manifest(&workspace, serde_json::json!({ "@pnpm.e2e/pkg-with-1-dep": "100.0.0" }));
    write_workspace_yaml(&workspace, "nodeLinker: hoisted\n");

    let output = pacquet_at(&workspace).with_args(["install"]).output().expect("run pnpm install");
    assert!(output.status.success(), "install failed: {output:?}");
    let stdout = String::from_utf8(output.stdout).expect("stdout is UTF-8");
    assert!(
        stdout.contains("Progress: resolved 2, reused 0, downloaded 2, added 2, done"),
        "stdout:\n{stdout}",
    );

    drop((root, mock_instance));
}

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

/// Workspace-wide hoisting under the frozen path. When the root
/// importer and a workspace project pin conflicting versions of one
/// name, the root's version wins the top-level slot — root deps rank
/// first in the hoister's preference order — and the project's
/// version nests under its own `node_modules`.
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

/// A cached occurrence of an npm-aliased package in a peer cycle must
/// reuse the fully walked occurrence's final depPath. Otherwise the
/// lockfile points at a peer-suffixed snapshot that was never emitted,
/// and the following frozen install cannot replay it.
#[test]
fn frozen_install_replays_a_cached_cyclic_alias_peer_snapshot() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "name": "cached-cyclic-alias-peer",
            "version": "1.0.0",
            "devDependencies": {
                "devtools": "npm:@pnpm.e2e/peer-a@1.0.0",
                "vite": "npm:@pnpm.e2e/foo@1.0.0",
                "vite-plus": "npm:@pnpm.e2e/pkg-with-1-dep@100.0.0"
            }
        })
        .to_string(),
    )
    .expect("write package.json");
    write_workspace_yaml(
        &workspace,
        concat!(
            "nodeLinker: hoisted\n",
            "packageExtensions:\n",
            "  '@pnpm.e2e/foo@1.0.0':\n",
            "    peerDependencies:\n",
            "      devtools: '*'\n",
            "    peerDependenciesMeta:\n",
            "      devtools:\n",
            "        optional: true\n",
            "  '@pnpm.e2e/peer-a@1.0.0':\n",
            "    peerDependencies:\n",
            "      vite: '*'\n",
            "  '@pnpm.e2e/pkg-with-1-dep@100.0.0':\n",
            "    dependencies:\n",
            "      '@pnpm.e2e/foo': 1.0.0\n",
        ),
    );

    pacquet.with_args(["install", "--lockfile-only", "--ignore-scripts"]).assert().success();
    pacquet_at(&workspace)
        .with_args(["install", "--frozen-lockfile", "--ignore-scripts"])
        .assert()
        .success();

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

/// TS: `install only the dependencies of the specified importer, when
/// node-linker is hoisted` (`multipleImporters.ts:87`). The subset
/// install lands the selected project's dependency at the workspace
/// root, and the wanted lockfile keeps the unselected importer's
/// entries. (Upstream leaves "the unselected dependency is absent" as a
/// TODO — the hoisted linker materializes the full shared graph — so
/// only the positive assertions are pinned, matching upstream.)
#[test]
fn install_only_dependencies_of_specified_importer_with_hoisted_linker() {
    let fixture = WorkspaceFixture::new();
    fixture.append_workspace_yaml("nodeLinker: hoisted\n");
    fixture.project(
        "project-1",
        "project-1",
        ManifestDeps { prod: &[("@pnpm.e2e/foo", "1.0.0")], ..Default::default() },
    );
    fixture.project(
        "project-2",
        "project-2",
        ManifestDeps { prod: &[("@foo/no-deps", "1.0.0")], ..Default::default() },
    );

    fixture.run(["--filter", "project-1", "install"]);

    assert!(
        is_real_dir(&fixture.workspace, "node_modules/@pnpm.e2e/foo"),
        "the selected project's dependency must be hoisted to the workspace root",
    );
    let wanted = fixture.wanted();
    assert_eq!(importer_version(&wanted, "packages/project-2", "@foo/no-deps"), "1.0.0");
}

/// TS: `run pre/postinstall scripts in a workspace that uses
/// node-linker=hoisted` (`lifecycleScripts.ts:718`). Two projects pin
/// `@pnpm.e2e/pre-and-postinstall-scripts-example@1` and two pin `@2`;
/// the hoisted layout keeps one version at the workspace root and
/// nests the other under its consumers, and the build step must run
/// the scripts at every materialized copy. This case retains frozen
/// reinstall coverage; fresh hoisted installs are covered below.
#[test]
fn run_pre_and_postinstall_scripts_in_a_workspace_with_hoisted_linker() {
    const SCRIPTS: &str = "@pnpm.e2e/pre-and-postinstall-scripts-example";
    let fixture = WorkspaceFixture::new();
    fixture.append_workspace_yaml(&format!(
        "nodeLinker: hoisted\nallowBuilds:\n  '{SCRIPTS}': true\n",
    ));
    let mut projects = Vec::new();
    for (dir, spec) in
        [("project-1", "1"), ("project-2", "1"), ("project-3", "2"), ("project-4", "2")]
    {
        projects.push(fixture.project(
            dir,
            dir,
            ManifestDeps { prod: &[(SCRIPTS, spec)], ..Default::default() },
        ));
    }
    fixture.run(["install", "--lockfile-only"]);

    fixture.run(["install", "--frozen-lockfile"]);

    assert_eq!(
        read_pkg_version(
            &fixture.workspace,
            "node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example"
        ),
        "1.0.0",
        "the majority-tie version must win the workspace-root slot, matching upstream",
    );
    for generated in ["generated-by-preinstall.js", "generated-by-postinstall.js"] {
        assert!(
            fixture.workspace.join("node_modules").join(SCRIPTS).join(generated).exists(),
            "the hoisted root copy must be built ({generated})",
        );
        // Only the versions that lost the root slot are nested, and
        // every nested copy must be built.
        for project in &projects[2..] {
            assert!(
                project.join("node_modules").join(SCRIPTS).join(generated).exists(),
                "every nested copy must be built ({generated})",
            );
        }
    }
    // Asserting the nested version too: a nested copy of the *root's*
    // version would satisfy the build checks above while still being the
    // wrong layout.
    for project in &projects[2..] {
        assert_eq!(
            read_pkg_version(project, &format!("node_modules/{SCRIPTS}")),
            "2.0.0",
            "the nested copy must be the version that lost the root slot",
        );
    }
    // The projects whose version won the root slot reach it by walking
    // up, so they must not carry a second copy of their own.
    for project in &projects[..2] {
        assert!(
            !project.join("node_modules").join(SCRIPTS).exists(),
            "a project on the hoisted version must not nest its own copy",
        );
    }
}

/// A version that lost the root slot and later wins it must leave no
/// nested copy behind: the stale entry would keep shadowing the root for
/// that project, which is the duplication this layout avoids.
#[test]
fn a_nested_copy_is_removed_once_its_version_wins_the_root_slot() {
    const SCRIPTS: &str = "@pnpm.e2e/pre-and-postinstall-scripts-example";
    let fixture = WorkspaceFixture::new();
    fixture.append_workspace_yaml(&format!(
        "nodeLinker: hoisted\nallowBuilds:\n  '{SCRIPTS}': true\n",
    ));
    let loser = fixture.project(
        "loser",
        "loser",
        ManifestDeps { prod: &[(SCRIPTS, "2")], ..Default::default() },
    );
    for dir in ["winner-a", "winner-b"] {
        fixture.project(dir, dir, ManifestDeps { prod: &[(SCRIPTS, "1")], ..Default::default() });
    }
    fixture.run(["install"]);
    assert_eq!(
        read_pkg_version(&loser, &format!("node_modules/{SCRIPTS}")),
        "2.0.0",
        "the minority version starts out nested",
    );

    // Flip the majority so the formerly nested version wins the root slot.
    for dir in ["winner-a", "winner-b"] {
        fixture.project(dir, dir, ManifestDeps { prod: &[(SCRIPTS, "2")], ..Default::default() });
    }
    fixture.run(["install"]);

    assert_eq!(
        read_pkg_version(&fixture.workspace, &format!("node_modules/{SCRIPTS}")),
        "2.0.0",
        "the new majority version must hold the root slot",
    );
    assert!(
        !loser.join("node_modules").join(SCRIPTS).exists(),
        "the stale nested copy must not survive the transition",
    );

    // The same must hold for a link left behind by an install that did
    // materialize the root-slot winner inside the project.
    let stale = loser.join("node_modules").join(SCRIPTS);
    fs::create_dir_all(stale.parent().expect("scope dir")).expect("create the scope dir");
    std::os::unix::fs::symlink(fixture.workspace.join("node_modules").join(SCRIPTS), &stale)
        .expect("plant a stale link");
    // The repeat-install short-circuit would report the unchanged
    // workspace up to date without ever reaching the linker.
    fs::remove_file(fixture.workspace.join("node_modules/.pnpm-workspace-state-v1.json"))
        .expect("remove the workspace state");
    fixture.run(["install"]);

    assert!(!stale.exists(), "a stale project-local link must not survive a reinstall");
}

/// TS: `overwriting (…@3.0.0 with …@latest)`
/// (`hoistedNodeLinker/install.ts:61`), on registry-mock fixtures:
/// re-adding at `@latest` replaces the on-disk hoisted directory with
/// the newly resolved version.
#[test]
fn overwriting_is_positive_with_latest() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_workspace_yaml(&workspace, "nodeLinker: hoisted\n");

    pacquet.with_args(["add", "@pnpm.e2e/dep-of-pkg-with-1-dep@100.0.0"]).assert().success();
    assert_eq!(
        read_pkg_version(&workspace, "node_modules/@pnpm.e2e/dep-of-pkg-with-1-dep"),
        "100.0.0",
    );

    pacquet_at(&workspace)
        .with_args(["add", "@pnpm.e2e/dep-of-pkg-with-1-dep@latest"])
        .assert()
        .success();
    let on_disk = read_pkg_version(&workspace, "node_modules/@pnpm.e2e/dep-of-pkg-with-1-dep");
    assert_ne!(on_disk, "100.0.0", "the hoisted directory must be overwritten with `latest`");
    let manifest = fs::read_to_string(workspace.join("package.json")).expect("read package.json");
    let manifest: serde_json::Value = serde_json::from_str(&manifest).expect("parse package.json");
    let spec = manifest["dependencies"]["@pnpm.e2e/dep-of-pkg-with-1-dep"]
        .as_str()
        .expect("dep recorded in the manifest");
    assert!(spec.contains(&on_disk), "manifest spec {spec:?} must pin the on-disk {on_disk}");

    drop((root, mock_instance));
}

/// TS: `overwriting existing files in node_modules`
/// (`hoistedNodeLinker/install.ts:83`): a pre-existing wrong occupant
/// (a symlink squatting the package's path) is replaced by the real
/// package.
#[test]
fn overwriting_existing_files_in_node_modules() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_workspace_yaml(&workspace, "nodeLinker: hoisted\n");

    fs::create_dir_all(workspace.join("node_modules")).expect("create node_modules");
    std::os::unix::fs::symlink(&workspace, workspace.join("node_modules/is-positive"))
        .expect("plant a wrong occupant symlink");

    pacquet.with_args(["add", "is-positive@1.0.0"]).assert().success();
    assert_eq!(read_pkg_version(&workspace, "node_modules/is-positive"), "1.0.0");
    assert!(
        is_real_dir(&workspace, "node_modules/is-positive"),
        "the squatting symlink must be replaced by the real package directory",
    );

    drop((root, mock_instance));
}

/// TS: `preserve subdeps on update` (`hoistedNodeLinker/install.ts:97`):
/// updating the parent replaces its directory but keeps the untouched
/// nested conflict copy.
#[test]
fn preserve_subdeps_on_update() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_workspace_yaml(&workspace, "nodeLinker: hoisted\n");

    pacquet
        .with_args(["add", "@pnpm.e2e/foobarqar@1.0.0", "@pnpm.e2e/bar@100.1.0"])
        .assert()
        .success();
    assert_eq!(read_pkg_version(&workspace, "node_modules/@pnpm.e2e/bar"), "100.1.0");
    assert_eq!(
        read_pkg_version(&workspace, "node_modules/@pnpm.e2e/foobarqar/node_modules/@pnpm.e2e/bar"),
        "100.0.0",
    );

    pacquet_at(&workspace).with_args(["add", "@pnpm.e2e/foobarqar@1.0.1"]).assert().success();
    assert_eq!(read_pkg_version(&workspace, "node_modules/@pnpm.e2e/bar"), "100.1.0");
    assert_eq!(read_pkg_version(&workspace, "node_modules/@pnpm.e2e/foobarqar"), "1.0.1");
    assert_eq!(
        read_pkg_version(&workspace, "node_modules/@pnpm.e2e/foobarqar/node_modules/@pnpm.e2e/bar"),
        "100.0.0",
        "the nested conflict copy must survive the parent's update",
    );

    drop((root, mock_instance));
}

/// TS: `adding a new dependency to one of the workspace projects`
/// (`hoistedNodeLinker/install.ts:119`): the added dep hoists into the
/// shared root `node_modules` and only the targeted member's manifest
/// changes.
#[test]
fn adding_a_new_dependency_to_a_workspace_project() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_workspace_yaml(
        &workspace,
        "nodeLinker: hoisted\npackages:\n  - project-1\n  - project-2\n",
    );
    fs::write(workspace.join("package.json"), serde_json::json!({ "name": "root" }).to_string())
        .expect("write root package.json");
    for (name, deps) in [
        ("project-1", serde_json::json!({ "@pnpm.e2e/bar": "100.0.0" })),
        ("project-2", serde_json::json!({ "@pnpm.e2e/foobarqar": "1.0.0" })),
    ] {
        fs::create_dir_all(workspace.join(name)).expect("create member dir");
        fs::write(
            workspace.join(name).join("package.json"),
            serde_json::json!({ "name": name, "version": "1.0.0", "dependencies": deps })
                .to_string(),
        )
        .expect("write member package.json");
    }
    pacquet.with_arg("install").assert().success();

    pacquet_at(&workspace.join("project-1"))
        .with_args(["add", "--save-dev", "is-negative@1.0.0"])
        .assert()
        .success();

    let manifest = fs::read_to_string(workspace.join("project-1/package.json"))
        .expect("read project-1 package.json");
    let manifest: serde_json::Value = serde_json::from_str(&manifest).expect("parse manifest");
    assert_eq!(manifest["dependencies"], serde_json::json!({ "@pnpm.e2e/bar": "100.0.0" }));
    assert_eq!(manifest["devDependencies"], serde_json::json!({ "is-negative": "1.0.0" }));
    assert_eq!(read_pkg_version(&workspace, "node_modules/@pnpm.e2e/bar"), "100.0.0");
    assert_eq!(read_pkg_version(&workspace, "node_modules/is-negative"), "1.0.0");

    drop((root, mock_instance));
}

/// TS: `installing the same package with alias and no alias`
/// (`hoistedNodeLinker/install.ts:172`): the aliased dir, the
/// real-named dir, and the aliasing package all materialize, at one
/// underlying version.
#[test]
fn installing_same_package_with_alias_and_no_alias() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_workspace_yaml(&workspace, "nodeLinker: hoisted\n");

    pacquet
        .with_args([
            "add",
            "@pnpm.e2e/pkg-with-1-aliased-dep@100.0.0",
            "@pnpm.e2e/dep-of-pkg-with-1-dep@^100.0.0",
        ])
        .assert()
        .success();

    assert_eq!(
        read_pkg_version(&workspace, "node_modules/@pnpm.e2e/pkg-with-1-aliased-dep"),
        "100.0.0",
    );
    let direct = read_pkg_version(&workspace, "node_modules/@pnpm.e2e/dep-of-pkg-with-1-dep");
    let aliased = read_pkg_version(&workspace, "node_modules/dep");
    assert_eq!(direct, aliased, "alias and real name must resolve to one version");
    assert_eq!(direct, "100.1.0");

    drop((root, mock_instance));
}

/// TS: `installing with hoisted node-linker a package that is a peer
/// dependency of itself` (`hoistedNodeLinker/install.ts:329`,
/// pnpm/pnpm#8854): the self-peer must not be recorded as a
/// `peerDependencies` entry in the lockfile.
#[test]
fn package_that_is_peer_dependency_of_itself() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_workspace_yaml(&workspace, "nodeLinker: hoisted\n");

    pacquet.with_args(["add", "@pnpm.e2e/peer-of-itself@1.0.0"]).assert().success();
    assert!(workspace.join("node_modules/@pnpm.e2e/peer-of-itself").exists());

    let lockfile = fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read lockfile");
    let lockfile: pnpm_lockfile::Lockfile =
        serde_saphyr::from_str(&lockfile).expect("parse pnpm-lock.yaml");
    let packages = lockfile.packages.expect("lockfile has a packages section");
    let (_, metadata) = packages
        .iter()
        .find(|(key, _)| key.to_string() == "@pnpm.e2e/peer-of-itself@1.0.0")
        .expect("peer-of-itself is recorded in packages");
    assert!(
        metadata.peer_dependencies.is_none(),
        "a self-peer must not be recorded as a peerDependencies entry: {:?}",
        metadata.peer_dependencies,
    );

    drop((root, mock_instance));
}

/// TS: `run pre/postinstall scripts. bin files should be linked in a
/// hoisted node_modules` (`hoistedNodeLinker/install.ts:187`).
#[test]
fn run_pre_and_postinstall_scripts_and_link_bins() {
    const SCRIPTS: &str = "@pnpm.e2e/pre-and-postinstall-scripts-example";
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_manifest(&workspace, serde_json::json!({ SCRIPTS: "1.0.0" }));
    write_workspace_yaml(
        &workspace,
        &format!("nodeLinker: hoisted\nallowBuilds:\n  '{SCRIPTS}': true\n"),
    );

    pacquet.with_arg("install").assert().success();

    let package_dir = workspace.join("node_modules").join(SCRIPTS);
    assert!(!package_dir.join("generated-by-prepare.js").exists());
    assert!(package_dir.join("generated-by-preinstall.js").exists());
    assert!(package_dir.join("generated-by-postinstall.js").exists());

    drop((root, mock_instance));
}

/// TS: `running install scripts in a workspace that has no root project`
/// (`hoistedNodeLinker/install.ts:210`).
#[test]
fn running_install_scripts_in_workspace_without_root_project() {
    const SCRIPTS: &str = "@pnpm.e2e/pre-and-postinstall-scripts-example";
    let fixture = WorkspaceFixture::new();
    fixture.append_workspace_yaml(&format!(
        "nodeLinker: hoisted\nallowBuilds:\n  '{SCRIPTS}': true\n",
    ));
    fixture.project(
        "project-1",
        "project-1",
        ManifestDeps { prod: &[(SCRIPTS, "1.0.0")], ..Default::default() },
    );

    fixture.run(["install"]);

    assert!(
        fixture
            .workspace
            .join("node_modules")
            .join(SCRIPTS)
            .join("generated-by-preinstall.js")
            .exists(),
    );
}

/// TS: `linking bins of local projects when node-linker is set to
/// hoisted` (`hoistedNodeLinker/install.ts:262`).
#[test]
fn linking_bins_of_local_projects() {
    let fixture = WorkspaceFixture::new();
    fixture.append_workspace_yaml("nodeLinker: hoisted\n");
    let consumer = fixture.project(
        "project-1",
        "project-1",
        ManifestDeps { prod: &[("project-2", "workspace:*")], ..Default::default() },
    );
    let provider = fixture.project("project-2", "project-2", ManifestDeps::default());
    let mut provider_manifest = read_manifest(&provider);
    provider_manifest["bin"] = serde_json::json!({ "project-2": "index.js" });
    write_manifest_value(&provider, &provider_manifest);
    fs::write(provider.join("index.js"), "#!/usr/bin/env node\nconsole.log('hello')\n")
        .expect("write project bin");

    fixture.run(["install"]);

    assert!(consumer.join("node_modules/.bin/project-2").exists());
}

/// The hoisted linker turns `preferSymlinkedExecutables` on by
/// default, so on Unix `.bin` entries are symlinks to the bin file
/// instead of shell shims — pnpm's `nodeLinker: hoisted` behavior. An
/// explicit `preferSymlinkedExecutables: false` restores the shims.
#[test]
#[cfg_attr(target_os = "windows", ignore = "preferSymlinkedExecutables is inert on Windows")]
fn hoisted_linker_symlinks_bins_by_default() {
    for (yaml, expect_symlink) in [
        ("nodeLinker: hoisted\n", true),
        ("nodeLinker: hoisted\npreferSymlinkedExecutables: false\n", false),
    ] {
        let fixture = WorkspaceFixture::new();
        fixture.append_workspace_yaml(yaml);
        let consumer = fixture.project(
            "project-1",
            "project-1",
            ManifestDeps { prod: &[("project-2", "workspace:*")], ..Default::default() },
        );
        let provider = fixture.project("project-2", "project-2", ManifestDeps::default());
        let mut provider_manifest = read_manifest(&provider);
        provider_manifest["bin"] = serde_json::json!({ "project-2": "index.js" });
        write_manifest_value(&provider, &provider_manifest);
        fs::write(provider.join("index.js"), "#!/usr/bin/env node\nconsole.log('hello')\n")
            .expect("write project bin");

        fixture.run(["install"]);

        let bin = consumer.join("node_modules/.bin/project-2");
        let is_symlink =
            fs::symlink_metadata(&bin).expect("bin must exist").file_type().is_symlink();
        assert_eq!(is_symlink, expect_symlink, "yaml: {yaml}");
    }
}

/// TS: `run pre/postinstall scripts in a project that uses
/// node-linker=hoisted. Should not fail on repeat install`
/// (`lifecycleScripts.ts:825`).
#[test]
fn lifecycle_scripts_do_not_fail_on_repeat_hoisted_install() {
    const SCRIPTS: &str = "@pnpm.e2e/pre-and-postinstall-scripts-example";
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_manifest(&workspace, serde_json::json!({ SCRIPTS: "1.0.0" }));
    write_workspace_yaml(
        &workspace,
        &format!(
            "nodeLinker: hoisted\nsideEffectsCacheRead: true\nsideEffectsCacheWrite: true\nallowBuilds:\n  '{SCRIPTS}': true\n",
        ),
    );
    pacquet.with_arg("install").assert().success();

    write_manifest(
        &workspace,
        serde_json::json!({
            SCRIPTS: "1.0.0",
            "example": "npm:@pnpm.e2e/pre-and-postinstall-scripts-example@2.0.0",
        }),
    );
    pacquet_in(&workspace).with_arg("install").assert().success();

    for package_dir in
        [workspace.join("node_modules").join(SCRIPTS), workspace.join("node_modules/example")]
    {
        assert!(package_dir.join("generated-by-preinstall.js").exists());
        assert!(package_dir.join("generated-by-postinstall.js").exists());
    }

    drop((root, mock_instance));
}

/// A hoist pattern is inert under `nodeLinker: hoisted`. The isolated
/// hoist writes symlinks into `<virtual_store>/node_modules`, and the
/// hoisted linker builds no virtual store to point them at, so the plan
/// is suppressed rather than producing links to nowhere.
///
/// Both paths must agree: the frozen path suppresses the plan by passing
/// `is_hoisted` into `compute_hoist_plan`, while the fresh path only
/// reaches that call inside its isolated branch. This pins the shared
/// outcome so the two cannot drift apart.
#[test]
fn hoist_patterns_are_inert_under_the_hoisted_linker() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_manifest(&workspace, serde_json::json!({ "send": "0.17.2" }));
    write_workspace_yaml(
        &workspace,
        "nodeLinker: hoisted\npublicHoistPattern:\n  - '*'\nhoistPattern:\n  - '*'\n",
    );

    let assert_no_isolated_hoist = |stage: &str| {
        assert!(
            is_real_dir(&workspace, "node_modules/send"),
            "{stage}: the hoisted linker still lands real directories",
        );
        assert!(
            !workspace.join("node_modules/.pnpm/node_modules").exists(),
            "{stage}: no private-hoist dir, because there is no virtual store to hoist into",
        );
        let modules_yaml = fs::read_to_string(workspace.join("node_modules/.modules.yaml"))
            .expect("read .modules.yaml");
        assert!(
            modules_yaml.contains(r#""hoistedDependencies": {}"#),
            "{stage}: the isolated hoist recorded nothing; got:\n{modules_yaml}",
        );
    };

    pacquet.with_args(["install"]).assert().success();
    assert_no_isolated_hoist("fresh");

    fs_remove_dir_all(&workspace.join("node_modules"));
    pacquet_at(&workspace).with_arg("install").with_arg("--frozen-lockfile").assert().success();
    assert_no_isolated_hoist("frozen");

    drop((root, mock_instance));
}

/// The hoisted linker imports directly into the flat `node_modules/`
/// rather than into a virtual-store slot, so it needs its own guard
/// that a `file:` dependency's copy is retaken when the source moves.
#[test]
fn a_directory_dependency_is_recopied_under_the_hoisted_linker() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_workspace_yaml(&workspace, "nodeLinker: hoisted\n");

    let local = workspace.join("local-pkg");
    fs::create_dir_all(&local).expect("create the local package dir");
    let write_local = |marker: &str| {
        fs::write(
            local.join("package.json"),
            serde_json::json!({ "name": "local-pkg", "version": "1.0.0" }).to_string(),
        )
        .expect("write the local package.json");
        fs::write(local.join("marker.txt"), marker).expect("write the marker");
    };
    write_local("first");
    write_manifest(&workspace, serde_json::json!({ "local-pkg": "file:./local-pkg" }));

    pacquet.with_arg("install").assert().success();
    let installed_marker = workspace.join("node_modules/local-pkg/marker.txt");
    assert_eq!(
        fs::read_to_string(&installed_marker).expect("read the installed marker"),
        "first",
        "the first install should materialize the directory dependency",
    );

    write_local("second");
    pacquet_in(&workspace).with_arg("install").assert().success();

    assert_eq!(
        fs::read_to_string(&installed_marker).expect("read the installed marker"),
        "second",
        "the second install should re-copy the directory into node_modules",
    );

    drop((root, mock_instance));
}

/// Two projects on the same `@pnpm.e2e/abc@1.0.0` but on different
/// `peer-a` versions give the lockfile two peer variants of one package
/// version. pnpm hoists such variants into a single root copy; nesting
/// a second, identical copy under the project only costs disk and
/// install time.
///
/// Collapsing the variants leaves only one of the two snapshot keys in
/// the dep graph, so the package map is asserted too: a project that
/// declared the *other* variant must still resolve the dependency it
/// declared, through the one copy at the root.
#[test]
fn peer_variants_of_one_version_share_the_root_slot() {
    let fixture = WorkspaceFixture::new();
    fixture.append_workspace_yaml("nodeLinker: hoisted\n");
    let deps_with_peer_a_1_0_0 = [
        ("@pnpm.e2e/abc", "1.0.0"),
        ("@pnpm.e2e/peer-a", "1.0.0"),
        ("@pnpm.e2e/peer-b", "1.0.0"),
        ("@pnpm.e2e/peer-c", "1.0.0"),
    ];
    let deps_with_peer_a_1_0_1 = [
        ("@pnpm.e2e/abc", "1.0.0"),
        ("@pnpm.e2e/peer-a", "1.0.1"),
        ("@pnpm.e2e/peer-b", "1.0.0"),
        ("@pnpm.e2e/peer-c", "1.0.0"),
    ];
    let on_peer_a_1_0_0 = fixture.project(
        "a",
        "a",
        ManifestDeps { prod: &deps_with_peer_a_1_0_0, ..Default::default() },
    );
    let on_peer_a_1_0_1 = fixture.project(
        "b",
        "b",
        ManifestDeps { prod: &deps_with_peer_a_1_0_1, ..Default::default() },
    );

    fixture.run(["install"]);

    assert!(
        is_real_dir(&fixture.workspace, "node_modules/@pnpm.e2e/abc"),
        "the shared version holds the root slot",
    );
    for project in [&on_peer_a_1_0_0, &on_peer_a_1_0_1] {
        assert!(
            !project.join("node_modules/@pnpm.e2e/abc").exists(),
            "a peer variant of the root version must not nest its own copy: {project:?}",
        );
    }

    let package_map: serde_json::Value =
        serde_json::from_str(&package_map_contents(&fixture.workspace))
            .expect("parse the package map");
    for project in ["a", "b"] {
        let dependency_id = package_map["packages"][format!("../packages/{project}")]
            ["dependencies"]["@pnpm.e2e/abc"]
            .as_str()
            .unwrap_or_else(|| panic!("packages/{project} declares @pnpm.e2e/abc"));
        assert_eq!(
            package_map["packages"][dependency_id]["url"], "./@pnpm.e2e/abc",
            "packages/{project} resolves @pnpm.e2e/abc through the root copy",
        );
    }
}
