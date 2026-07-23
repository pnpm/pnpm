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
//! Cases that depend on features pacquet hasn't built yet live in
//! [`known_failures`] below with
//! [`pacquet_testing_utils::allow_known_failure`] gating the assertion
//! against the not-yet-implemented subject under test.

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
        // Pacquet's hoisted linker materializes each project's direct
        // dep under the project as well — upstream nests a copy only
        // for the version that lost the root slot (see
        // `known_failures::hoisted_workspace_layout_does_not_duplicate_root_version`).
        // Every copy that is materialized must be built.
        for project in &projects {
            assert!(
                project.join("node_modules").join(SCRIPTS).join(generated).exists(),
                "every materialized copy must be built ({generated})",
            );
        }
    }
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
    let lockfile: pacquet_lockfile::Lockfile =
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

mod known_failures {
    //! Hoisted-node-linker cases blocked on features pacquet hasn't
    //! built yet. Each stubs the not-yet-built subject through
    //! [`pacquet_testing_utils::allow_known_failure`] so the test exits
    //! early rather than masking a real bug. The `pnpm add` / update
    //! manifest-mutation cases formerly stubbed here are real tests in
    //! the parent module since the prune-stale-modules reconciliation
    //! landed.

    use pacquet_testing_utils::{
        allow_known_failure,
        known_failure::{KnownFailure, KnownResult},
    };

    fn hoisted_workspace_duplicate_materialization() -> KnownResult<()> {
        Err(KnownFailure::new(
            "Pacquet's hoisted linker materializes each workspace \
             project's direct dependency under the project's own \
             `node_modules` even when the same version already won the \
             workspace-root slot. Upstream nests a copy only for \
             versions that lost the root slot \
             (`lifecycleScripts.ts:718` asserts the hoisted-version \
             consumers have no nested copy).",
        ))
    }

    /// The layout tail of TS `run pre/postinstall scripts in a
    /// workspace that uses node-linker=hoisted`
    /// (`lifecycleScripts.ts:718`); the script-execution half is the
    /// real [`super::run_pre_and_postinstall_scripts_in_a_workspace_with_hoisted_linker`].
    #[test]
    fn hoisted_workspace_layout_does_not_duplicate_root_version() {
        allow_known_failure!(hoisted_workspace_duplicate_materialization());
    }
}
