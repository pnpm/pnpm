use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pacquet_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use serde_json::Value;
use std::{ffi::OsStr, fmt::Write as _, fs, path::Path, process::Command};
use tempfile::TempDir;

const IS_POSITIVE_PATCH: &str = include_str!(
    "../../../../pnpm11/installing/deps-installer/test/fixtures/patch-pkg/is-positive@1.0.0.patch"
);

fn setup_installed() -> (TempDir, std::path::PathBuf, AddMockedRegistry) {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "dependencies": {
                "is-positive": "1.0.0",
            },
        })
        .to_string(),
    )
    .expect("write package.json");
    pacquet(&workspace, ["install"]).assert().success();
    (root, workspace, npmrc_info)
}

fn setup_installed_workspace_project()
-> (TempDir, std::path::PathBuf, std::path::PathBuf, AddMockedRegistry) {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let workspace_yaml_path = workspace.join("pnpm-workspace.yaml");
    let mut workspace_yaml =
        fs::read_to_string(&workspace_yaml_path).expect("read pnpm-workspace.yaml");
    if !workspace_yaml.ends_with('\n') {
        workspace_yaml.push('\n');
    }
    workspace_yaml.push_str("packages:\n  - 'packages/*'\n");
    fs::write(&workspace_yaml_path, workspace_yaml).expect("write pnpm-workspace.yaml");
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "name": "root",
            "version": "0.0.0",
            "private": true,
        })
        .to_string(),
    )
    .expect("write root package.json");
    let app_dir = workspace.join("packages/app");
    fs::create_dir_all(&app_dir).expect("create workspace app");
    fs::write(
        app_dir.join("package.json"),
        serde_json::json!({
            "name": "app",
            "version": "0.0.0",
            "dependencies": {
                "is-positive": "1.0.0",
            },
        })
        .to_string(),
    )
    .expect("write app package.json");
    pacquet(&workspace, ["install"]).assert().success();
    (root, workspace, app_dir, npmrc_info)
}

fn setup_configured_patch(
    patch_key: &str,
    patch_file_name: &str,
) -> (TempDir, std::path::PathBuf, AddMockedRegistry) {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "dependencies": {
                "is-positive": "1.0.0",
            },
        })
        .to_string(),
    )
    .expect("write package.json");
    fs::create_dir_all(workspace.join("patches")).expect("create patches dir");
    fs::write(workspace.join("patches").join(patch_file_name), IS_POSITIVE_PATCH)
        .expect("write patch file");
    let workspace_yaml_path = workspace.join("pnpm-workspace.yaml");
    let mut workspace_yaml =
        fs::read_to_string(&workspace_yaml_path).expect("read pnpm-workspace.yaml");
    if !workspace_yaml.ends_with('\n') {
        workspace_yaml.push('\n');
    }
    writeln!(&mut workspace_yaml, "patchedDependencies:\n  {patch_key}: patches/{patch_file_name}")
        .expect("append patchedDependencies");
    fs::write(&workspace_yaml_path, workspace_yaml).expect("write pnpm-workspace.yaml");
    (root, workspace, npmrc_info)
}

fn setup_patch_remove_project(
    entries: &[(&str, &str)],
) -> (TempDir, std::path::PathBuf, AddMockedRegistry) {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "dependencies": {
                "is-positive": "1.0.0",
            },
        })
        .to_string(),
    )
    .expect("write package.json");
    let workspace_yaml_path = workspace.join("pnpm-workspace.yaml");
    let mut workspace_yaml =
        fs::read_to_string(&workspace_yaml_path).expect("read pnpm-workspace.yaml");
    if !workspace_yaml.ends_with('\n') {
        workspace_yaml.push('\n');
    }
    workspace_yaml.push_str("patchedDependencies:\n");
    for (key, patch_file) in entries {
        writeln!(&mut workspace_yaml, "  {key}: {patch_file}")
            .expect("append patchedDependencies entry");
    }
    fs::write(&workspace_yaml_path, workspace_yaml).expect("write pnpm-workspace.yaml");
    (root, workspace, npmrc_info)
}

fn pacquet(workspace: &Path, args: impl IntoIterator<Item = impl AsRef<OsStr>>) -> Command {
    Command::cargo_bin("pacquet")
        .expect("find the pacquet binary")
        .with_current_dir(workspace)
        .with_args(args)
}

fn patch_state(workspace: &Path) -> Value {
    let state_path = workspace.join("node_modules/.pnpm_patches/state.json");
    serde_json::from_str(&fs::read_to_string(state_path).expect("read patch state"))
        .expect("parse patch state")
}

fn write_patch_edit(edit_dir: &Path, marker: &str) {
    fs::write(
        edit_dir.join("index.js"),
        format!("module.exports = function () {{ return {marker:?} }}\n"),
    )
    .expect("edit package file");
}

fn remove_dir_if_exists(path: &Path) {
    match fs::remove_dir_all(path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => panic!("remove {}: {error}", path.display()),
    }
}

#[test]
fn patch_errors_when_package_is_missing() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let output = pacquet(&workspace, ["patch", "--reporter=silent"]).output().expect("run patch");

    assert!(!output.status.success(), "patch without package should fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("requires the package name"), "stderr: {stderr}");

    drop((root, mock_instance));
}

#[test]
fn patch_missing_package_name_takes_precedence_over_edit_dir_checks() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    let edit_dir = workspace.join("custom-edit");
    fs::create_dir_all(&edit_dir).expect("create edit dir");
    fs::write(edit_dir.join("index.js"), "already here").expect("seed edit dir");

    let output = pacquet(
        &workspace,
        ["patch", "--edit-dir", edit_dir.to_str().expect("utf8 edit dir"), "--reporter=silent"],
    )
    .output()
    .expect("run patch");

    assert!(!output.status.success(), "patch without package should fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("requires the package name"), "stderr: {stderr}");
    assert!(!stderr.contains("already exists"), "stderr: {stderr}");

    drop((root, mock_instance));
}

#[test]
fn patch_errors_when_requested_version_is_not_installed() {
    let (root, workspace, npmrc_info) = setup_installed();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let output = pacquet(&workspace, ["patch", "is-positive@2.0.0", "--reporter=silent"])
        .output()
        .expect("run patch");

    assert!(!output.status.success(), "missing installed version should fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("ERR_PNPM_PATCH_VERSION_NOT_FOUND"), "stderr: {stderr}");
    assert!(stderr.contains("1.0.0"), "stderr: {stderr}");

    drop((root, mock_instance));
}

#[test]
fn install_level_exact_version_patch_applies_with_frozen_reinstall() {
    let (root, workspace, npmrc_info) =
        setup_configured_patch("is-positive@1.0.0", "is-positive@1.0.0.patch");
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    pacquet(&workspace, ["install", "--reporter=silent"]).assert().success();

    let installed =
        fs::read_to_string(workspace.join("node_modules/is-positive/index.js")).unwrap();
    assert!(installed.contains("// patched"), "installed: {installed}");
    let lockfile = fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("lockfile");
    assert!(lockfile.contains("patchedDependencies:"), "lockfile: {lockfile}");
    assert!(lockfile.contains("is-positive@1.0.0:"), "lockfile: {lockfile}");

    remove_dir_if_exists(&workspace.join("node_modules"));
    pacquet(&workspace, ["install", "--frozen-lockfile", "--reporter=silent"]).assert().success();
    let replayed = fs::read_to_string(workspace.join("node_modules/is-positive/index.js")).unwrap();
    assert!(replayed.contains("// patched"), "replayed: {replayed}");

    drop((root, mock_instance));
}

#[test]
fn install_level_range_patch_applies_with_frozen_reinstall() {
    let (root, workspace, npmrc_info) =
        setup_configured_patch("is-positive@1", "is-positive@1.patch");
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    pacquet(&workspace, ["install", "--reporter=silent"]).assert().success();

    let installed =
        fs::read_to_string(workspace.join("node_modules/is-positive/index.js")).unwrap();
    assert!(installed.contains("// patched"), "installed: {installed}");
    let lockfile = fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("lockfile");
    assert!(lockfile.contains("patchedDependencies:"), "lockfile: {lockfile}");
    assert!(lockfile.contains("is-positive@1:"), "lockfile: {lockfile}");

    remove_dir_if_exists(&workspace.join("node_modules"));
    pacquet(&workspace, ["install", "--frozen-lockfile", "--reporter=silent"]).assert().success();
    let replayed = fs::read_to_string(workspace.join("node_modules/is-positive/index.js")).unwrap();
    assert!(replayed.contains("// patched"), "replayed: {replayed}");

    drop((root, mock_instance));
}

#[test]
fn patch_commit_exact_version_writes_patch_and_reinstalls() {
    let (root, workspace, npmrc_info) = setup_installed();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    pacquet(&workspace, ["patch", "is-positive@1.0.0", "--reporter=silent"]).assert().success();
    let edit_dir = workspace.join("node_modules/.pnpm_patches/is-positive@1.0.0");
    write_patch_edit(&edit_dir, "patched exact");

    pacquet(
        &workspace,
        ["patch-commit", edit_dir.to_str().expect("utf8 edit dir"), "--reporter=silent"],
    )
    .assert()
    .success();

    let workspace_yaml =
        fs::read_to_string(workspace.join("pnpm-workspace.yaml")).expect("workspace yaml");
    assert!(workspace_yaml.contains("is-positive@1.0.0: patches/is-positive@1.0.0.patch"));

    let patch_file = workspace.join("patches/is-positive@1.0.0.patch");
    let patch = fs::read_to_string(patch_file).expect("patch file");
    assert!(patch.contains("diff --git a/index.js b/index.js"), "patch: {patch}");
    assert!(patch.contains("patched exact"), "patch: {patch}");

    let installed =
        fs::read_to_string(workspace.join("node_modules/is-positive/index.js")).unwrap();
    assert!(installed.contains("patched exact"), "installed: {installed}");

    drop((root, mock_instance));
}

#[test]
fn patch_commit_bare_name_writes_apply_to_all_key() {
    let (root, workspace, npmrc_info) = setup_installed();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    pacquet(&workspace, ["patch", "is-positive", "--reporter=silent"]).assert().success();
    let edit_dir = workspace.join("node_modules/.pnpm_patches/is-positive@1.0.0");
    write_patch_edit(&edit_dir, "patched all");

    pacquet(
        &workspace,
        ["patch-commit", edit_dir.to_str().expect("utf8 edit dir"), "--reporter=silent"],
    )
    .assert()
    .success();

    let workspace_yaml =
        fs::read_to_string(workspace.join("pnpm-workspace.yaml")).expect("workspace yaml");
    assert!(workspace_yaml.contains("is-positive: patches/is-positive.patch"));
    assert!(workspace.join("patches/is-positive.patch").is_file());

    drop((root, mock_instance));
}

#[test]
fn patch_commit_workspace_project_shared_lockfile_updates_root_manifest_and_reinstalls() {
    let (root, workspace, app_dir, npmrc_info) = setup_installed_workspace_project();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    pacquet(&workspace, ["patch", "is-positive@1.0.0", "--reporter=silent"]).assert().success();
    let edit_dir = workspace.join("node_modules/.pnpm_patches/is-positive@1.0.0");
    write_patch_edit(&edit_dir, "patched workspace");

    pacquet(
        &workspace,
        ["patch-commit", edit_dir.to_str().expect("utf8 edit dir"), "--reporter=silent"],
    )
    .assert()
    .success();

    let workspace_yaml =
        fs::read_to_string(workspace.join("pnpm-workspace.yaml")).expect("workspace yaml");
    assert!(workspace_yaml.contains("packages:"), "workspace yaml: {workspace_yaml}");
    assert!(
        workspace_yaml.contains("is-positive@1.0.0: patches/is-positive@1.0.0.patch"),
        "workspace yaml: {workspace_yaml}",
    );

    let installed = fs::read_to_string(app_dir.join("node_modules/is-positive/index.js")).unwrap();
    assert!(installed.contains("patched workspace"), "installed: {installed}");

    drop((root, mock_instance));
}

#[test]
fn patch_commit_accepts_relative_patch_dir() {
    let (root, workspace, npmrc_info) = setup_installed();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    pacquet(&workspace, ["patch", "is-positive@1.0.0", "--reporter=silent"]).assert().success();
    let edit_dir = workspace.join("node_modules/.pnpm_patches/is-positive@1.0.0");
    write_patch_edit(&edit_dir, "patched relative");

    pacquet(
        &workspace,
        ["patch-commit", "node_modules/.pnpm_patches/is-positive@1.0.0", "--reporter=silent"],
    )
    .assert()
    .success();

    let patch =
        fs::read_to_string(workspace.join("patches/is-positive@1.0.0.patch")).expect("patch");
    assert!(patch.contains("patched relative"), "patch: {patch}");

    drop((root, mock_instance));
}

#[test]
fn patch_commit_custom_patches_dir_normalizes_path() {
    let (root, workspace, npmrc_info) = setup_installed();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    pacquet(&workspace, ["patch", "is-positive@1.0.0", "--reporter=silent"]).assert().success();
    let edit_dir = workspace.join("node_modules/.pnpm_patches/is-positive@1.0.0");
    write_patch_edit(&edit_dir, "patched custom dir");

    pacquet(
        &workspace,
        [
            "patch-commit",
            "--patches-dir",
            "ts/src/../custom-patches",
            edit_dir.to_str().expect("utf8 edit dir"),
            "--reporter=silent",
        ],
    )
    .assert()
    .success();

    let workspace_yaml =
        fs::read_to_string(workspace.join("pnpm-workspace.yaml")).expect("workspace yaml");
    assert!(
        workspace_yaml.contains("is-positive@1.0.0: ts/custom-patches/is-positive@1.0.0.patch"),
        "workspace yaml: {workspace_yaml}",
    );
    assert!(workspace.join("ts/custom-patches/is-positive@1.0.0.patch").is_file());

    drop((root, mock_instance));
}

#[test]
fn patch_commit_no_changes_does_not_create_patches_dir() {
    for reporter in [None, Some("--reporter=ndjson"), Some("--reporter=silent")] {
        let (root, workspace, npmrc_info) = setup_installed();
        let AddMockedRegistry { mock_instance, .. } = npmrc_info;

        pacquet(&workspace, ["patch", "is-positive@1.0.0", "--reporter=silent"]).assert().success();
        let edit_dir = workspace.join("node_modules/.pnpm_patches/is-positive@1.0.0");

        let mut patch_commit =
            pacquet(&workspace, ["patch-commit", edit_dir.to_str().expect("utf8 edit dir")]);
        if let Some(reporter) = reporter {
            patch_commit.arg(reporter);
        }
        let output = patch_commit.output().expect("run patch-commit");

        assert!(output.status.success(), "patch-commit with no changes should succeed");
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("No changes were found"), "stdout: {stdout}");
        assert!(!workspace.join("patches").exists(), "patches dir should not be created");

        drop((root, mock_instance));
    }
}

#[test]
fn patch_commit_errors_when_patch_dir_manifest_is_missing() {
    let (root, workspace, npmrc_info) = setup_installed();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let output = pacquet(&workspace, ["patch-commit", "missing-edit-dir", "--reporter=silent"])
        .output()
        .expect("run patch-commit");

    assert!(!output.status.success(), "missing patch dir should fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Failed to read package manifest"), "stderr: {stderr}");

    drop((root, mock_instance));
}

#[test]
fn patch_commit_errors_when_manifest_version_no_longer_matches_installed_patch_target() {
    let (root, workspace, npmrc_info) = setup_installed();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    pacquet(&workspace, ["patch", "is-positive@1.0.0", "--reporter=silent"]).assert().success();
    let edit_dir = workspace.join("node_modules/.pnpm_patches/is-positive@1.0.0");
    fs::write(
        edit_dir.join("package.json"),
        serde_json::json!({
            "name": "is-positive",
            "version": "2.0.0",
        })
        .to_string(),
    )
    .expect("rewrite patch manifest");

    let output = pacquet(
        &workspace,
        ["patch-commit", edit_dir.to_str().expect("utf8 edit dir"), "--reporter=silent"],
    )
    .output()
    .expect("run patch-commit");

    assert!(!output.status.success(), "mismatched manifest version should fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("ERR_PNPM_PATCH_VERSION_NOT_FOUND"), "stderr: {stderr}");
    assert!(stderr.contains("current lockfile"), "stderr: {stderr}");
    assert!(stderr.contains("is-positive@2.0.0"), "stderr: {stderr}");

    drop((root, mock_instance));
}

#[test]
fn patch_commit_reports_patches_dir_create_errors() {
    let (root, workspace, npmrc_info) = setup_installed();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    pacquet(&workspace, ["patch", "is-positive@1.0.0", "--reporter=silent"]).assert().success();
    let edit_dir = workspace.join("node_modules/.pnpm_patches/is-positive@1.0.0");
    write_patch_edit(&edit_dir, "create patches dir error");
    fs::write(workspace.join("not-a-dir"), "").expect("create patches-dir file");

    let output = pacquet(
        &workspace,
        [
            "patch-commit",
            "--patches-dir",
            "not-a-dir",
            edit_dir.to_str().expect("utf8 edit dir"),
            "--reporter=silent",
        ],
    )
    .output()
    .expect("run patch-commit");

    assert!(!output.status.success(), "file patches dir should fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Failed to create patches directory"), "stderr: {stderr}");

    drop((root, mock_instance));
}

#[test]
fn patch_commit_reports_patch_file_write_errors() {
    let (root, workspace, npmrc_info) = setup_installed();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    pacquet(&workspace, ["patch", "is-positive@1.0.0", "--reporter=silent"]).assert().success();
    let edit_dir = workspace.join("node_modules/.pnpm_patches/is-positive@1.0.0");
    write_patch_edit(&edit_dir, "write patch error");
    fs::create_dir_all(workspace.join("patches/is-positive@1.0.0.patch"))
        .expect("create directory at patch file path");

    let output = pacquet(
        &workspace,
        ["patch-commit", edit_dir.to_str().expect("utf8 edit dir"), "--reporter=silent"],
    )
    .output()
    .expect("run patch-commit");

    assert!(!output.status.success(), "directory patch path should fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Failed to write patch file"), "stderr: {stderr}");

    drop((root, mock_instance));
}

#[test]
fn patch_workflow_runs_with_default_ndjson_and_silent_reporters() {
    for reporter in [None, Some("--reporter=ndjson"), Some("--reporter=silent")] {
        let (root, workspace, npmrc_info) = setup_installed();
        let AddMockedRegistry { mock_instance, .. } = npmrc_info;
        let marker = match reporter {
            None => "patched default reporter",
            Some("--reporter=ndjson") => "patched ndjson reporter",
            Some("--reporter=silent") => "patched silent reporter",
            Some(other) => panic!("unexpected reporter {other}"),
        };

        let mut patch_cmd = pacquet(&workspace, ["patch", "is-positive@1.0.0"]);
        if let Some(reporter) = reporter {
            patch_cmd.arg(reporter);
        }
        patch_cmd.assert().success();

        let edit_dir = workspace.join("node_modules/.pnpm_patches/is-positive@1.0.0");
        write_patch_edit(&edit_dir, marker);

        let mut patch_commit_cmd =
            pacquet(&workspace, ["patch-commit", edit_dir.to_str().expect("utf8 edit dir")]);
        if let Some(reporter) = reporter {
            patch_commit_cmd.arg(reporter);
        }
        patch_commit_cmd.assert().success();

        let installed =
            fs::read_to_string(workspace.join("node_modules/is-positive/index.js")).unwrap();
        assert!(installed.contains(marker), "installed: {installed}");
        assert!(workspace.join("patches/is-positive@1.0.0.patch").is_file());

        let mut patch_remove_cmd = pacquet(&workspace, ["patch-remove", "is-positive@1.0.0"]);
        if let Some(reporter) = reporter {
            patch_remove_cmd.arg(reporter);
        }
        patch_remove_cmd.assert().success();

        let workspace_yaml =
            fs::read_to_string(workspace.join("pnpm-workspace.yaml")).expect("workspace yaml");
        assert!(
            !workspace_yaml.contains("patchedDependencies:"),
            "workspace yaml: {workspace_yaml}",
        );
        assert!(!workspace.join("patches/is-positive@1.0.0.patch").exists());

        drop((root, mock_instance));
    }
}

#[test]
fn patch_reuses_existing_exact_patch_file_by_default() {
    let (root, workspace, npmrc_info) = setup_installed();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    pacquet(&workspace, ["patch", "is-positive@1.0.0", "--reporter=silent"]).assert().success();
    let edit_dir = workspace.join("node_modules/.pnpm_patches/is-positive@1.0.0");
    write_patch_edit(&edit_dir, "reused patch");
    pacquet(
        &workspace,
        ["patch-commit", edit_dir.to_str().expect("utf8 edit dir"), "--reporter=silent"],
    )
    .assert()
    .success();
    fs::remove_dir_all(&edit_dir).expect("remove edit dir");

    pacquet(&workspace, ["patch", "is-positive@1.0.0", "--reporter=silent"]).assert().success();

    let edited = fs::read_to_string(edit_dir.join("index.js")).expect("edit dir index");
    assert!(edited.contains("reused patch"), "edited: {edited}");

    drop((root, mock_instance));
}

#[test]
fn patch_ignore_existing_skips_existing_patch_file() {
    let (root, workspace, npmrc_info) = setup_installed();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    pacquet(&workspace, ["patch", "is-positive@1.0.0", "--reporter=silent"]).assert().success();
    let edit_dir = workspace.join("node_modules/.pnpm_patches/is-positive@1.0.0");
    write_patch_edit(&edit_dir, "ignored patch");
    pacquet(
        &workspace,
        ["patch-commit", edit_dir.to_str().expect("utf8 edit dir"), "--reporter=silent"],
    )
    .assert()
    .success();
    fs::remove_dir_all(&edit_dir).expect("remove edit dir");

    pacquet(&workspace, ["patch", "--ignore-existing", "is-positive@1.0.0", "--reporter=silent"])
        .assert()
        .success();

    let edited = fs::read_to_string(edit_dir.join("index.js")).expect("edit dir index");
    assert!(!edited.contains("ignored patch"), "edited: {edited}");

    drop((root, mock_instance));
}

#[test]
fn patch_errors_when_existing_patch_file_is_missing() {
    let (root, workspace, npmrc_info) = setup_installed();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    let workspace_yaml_path = workspace.join("pnpm-workspace.yaml");
    let mut workspace_yaml =
        fs::read_to_string(&workspace_yaml_path).expect("read pnpm-workspace.yaml");
    if !workspace_yaml.ends_with('\n') {
        workspace_yaml.push('\n');
    }
    workspace_yaml.push_str("patchedDependencies:\n  is-positive@1.0.0: patches/not-found.patch\n");
    fs::write(&workspace_yaml_path, workspace_yaml).expect("write pnpm-workspace.yaml");

    let output = pacquet(&workspace, ["patch", "is-positive@1.0.0", "--reporter=silent"])
        .output()
        .expect("run patch");

    assert!(!output.status.success(), "missing patch file should fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("ERR_PNPM_PATCH_FILE_NOT_FOUND"), "stderr: {stderr}");
    assert!(stderr.contains("Unable to find patch file"), "stderr: {stderr}");

    drop((root, mock_instance));
}

#[test]
fn patch_rejects_existing_patch_file_outside_patches_dir() {
    let (root, workspace, npmrc_info) = setup_installed();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    let outside_patch = workspace.parent().expect("workspace parent").join("outside.patch");
    fs::write(&outside_patch, IS_POSITIVE_PATCH).expect("write outside patch");
    let workspace_yaml_path = workspace.join("pnpm-workspace.yaml");
    let mut workspace_yaml =
        fs::read_to_string(&workspace_yaml_path).expect("read pnpm-workspace.yaml");
    if !workspace_yaml.ends_with('\n') {
        workspace_yaml.push('\n');
    }
    workspace_yaml.push_str("patchedDependencies:\n  is-positive@1.0.0: ../outside.patch\n");
    fs::write(&workspace_yaml_path, workspace_yaml).expect("write pnpm-workspace.yaml");

    let output = pacquet(&workspace, ["patch", "is-positive@1.0.0", "--reporter=silent"])
        .output()
        .expect("run patch");

    assert!(!output.status.success(), "outside patch file should fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("ERR_PNPM_PATCH_FILE_OUTSIDE_PATCHES_DIR"), "stderr: {stderr}");

    drop((root, mock_instance));
}

#[test]
fn patch_exact_version_creates_edit_dir_and_state() {
    let (root, workspace, npmrc_info) = setup_installed();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    pacquet(&workspace, ["patch", "is-positive@1.0.0", "--reporter=silent"]).assert().success();

    let edit_dir = workspace.join("node_modules/.pnpm_patches/is-positive@1.0.0");
    assert!(edit_dir.join("package.json").is_file(), "edit dir package.json exists");
    assert!(edit_dir.join("index.js").is_file(), "edit dir package files exist");

    let key = dunce::canonicalize(&edit_dir).expect("canonical edit dir").display().to_string();
    let state = patch_state(&workspace);
    assert_eq!(state[&key]["patchedPkg"], "is-positive@1.0.0");
    assert_eq!(state[&key]["applyToAll"], false);
    assert_eq!(state[&key]["packageKey"], "is-positive@1.0.0");

    drop((root, mock_instance));
}

#[cfg(unix)]
#[test]
fn patch_rejects_symlinked_default_edit_root() {
    let (root, workspace, npmrc_info) = setup_installed();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    let outside_dir = root.path().join("outside-patch-edits");
    fs::create_dir(&outside_dir).expect("create outside edit dir");
    std::os::unix::fs::symlink(&outside_dir, workspace.join("node_modules/.pnpm_patches"))
        .expect("symlink default patch edit root");

    let output = pacquet(&workspace, ["patch", "is-positive@1.0.0", "--reporter=silent"])
        .output()
        .expect("run patch");

    assert!(!output.status.success(), "symlinked default edit root should fail");
    assert!(
        !outside_dir.join("is-positive@1.0.0").exists(),
        "package files must not be extracted outside node_modules",
    );

    drop((root, mock_instance));
}

#[cfg(unix)]
#[test]
fn patch_commit_rejects_symlinked_patch_file_outside_patches_dir() {
    let (root, workspace, npmrc_info) = setup_installed();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    pacquet(&workspace, ["patch", "is-positive@1.0.0", "--reporter=silent"]).assert().success();
    let edit_dir = workspace.join("node_modules/.pnpm_patches/is-positive@1.0.0");
    write_patch_edit(&edit_dir, "symlink write attempt");
    let patches_dir = workspace.join("patches");
    fs::create_dir_all(&patches_dir).expect("create patches dir");
    let outside_target = workspace.parent().expect("workspace parent").join("outside.patch");
    fs::write(&outside_target, "outside original\n").expect("write outside target");
    std::os::unix::fs::symlink(&outside_target, patches_dir.join("is-positive@1.0.0.patch"))
        .expect("create patch symlink");

    let output = pacquet(
        &workspace,
        ["patch-commit", edit_dir.to_str().expect("utf8 edit dir"), "--reporter=silent"],
    )
    .output()
    .expect("run patch-commit");

    assert!(!output.status.success(), "symlinked patch file should fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("ERR_PNPM_PATCH_FILE_OUTSIDE_PATCHES_DIR"), "stderr: {stderr}");
    assert_eq!(
        fs::read_to_string(&outside_target).expect("read outside target"),
        "outside original\n",
    );

    drop((root, mock_instance));
}

#[test]
fn patch_bare_name_single_version_sets_apply_to_all() {
    let (root, workspace, npmrc_info) = setup_installed();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    pacquet(&workspace, ["patch", "is-positive", "--reporter=silent"]).assert().success();

    let edit_dir = workspace.join("node_modules/.pnpm_patches/is-positive@1.0.0");
    let key = dunce::canonicalize(&edit_dir).expect("canonical edit dir").display().to_string();
    let state = patch_state(&workspace);
    assert_eq!(state[&key]["patchedPkg"], "is-positive");
    assert_eq!(state[&key]["applyToAll"], true);
    assert_eq!(state[&key]["packageKey"], "is-positive@1.0.0");

    drop((root, mock_instance));
}

#[test]
fn patch_rejects_non_empty_edit_dir() {
    let (root, workspace, npmrc_info) = setup_installed();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    let edit_dir = workspace.join("custom-edit");
    fs::create_dir_all(&edit_dir).expect("create edit dir");
    fs::write(edit_dir.join("file.txt"), "already here").expect("seed edit dir");

    let output = pacquet(
        &workspace,
        [
            "patch",
            "--edit-dir",
            edit_dir.to_str().expect("utf8 path"),
            "is-positive@1.0.0",
            "--reporter=silent",
        ],
    )
    .output()
    .expect("run patch");

    assert!(!output.status.success(), "non-empty edit dir should fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("target directory already exists"), "stderr: {stderr}");

    drop((root, mock_instance));
}

#[test]
fn patch_accepts_empty_custom_edit_dir() {
    let (root, workspace, npmrc_info) = setup_installed();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    let edit_dir = workspace.join("custom-edit");
    fs::create_dir_all(&edit_dir).expect("create edit dir");

    pacquet(
        &workspace,
        [
            "patch",
            "--edit-dir",
            edit_dir.to_str().expect("utf8 path"),
            "is-positive@1.0.0",
            "--reporter=silent",
        ],
    )
    .assert()
    .success();

    assert!(edit_dir.join("package.json").is_file(), "custom edit dir package.json exists");
    assert!(edit_dir.join("index.js").is_file(), "custom edit dir package file exists");

    let key = dunce::canonicalize(&edit_dir).expect("canonical edit dir").display().to_string();
    let state = patch_state(&workspace);
    assert_eq!(state[&key]["patchedPkg"], "is-positive@1.0.0");
    assert_eq!(state[&key]["packageKey"], "is-positive@1.0.0");

    drop((root, mock_instance));
}

#[test]
fn patch_remove_removes_patch_file_manifest_entry_and_reinstalls() {
    let (root, workspace, npmrc_info) =
        setup_configured_patch("is-positive@1.0.0", "is-positive@1.0.0.patch");
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    pacquet(&workspace, ["install", "--reporter=silent"]).assert().success();
    let patched = fs::read_to_string(workspace.join("node_modules/is-positive/index.js")).unwrap();
    assert!(patched.contains("// patched"), "patched install: {patched}");

    pacquet(&workspace, ["patch-remove", "is-positive@1.0.0", "--reporter=silent"])
        .assert()
        .success();

    let workspace_yaml =
        fs::read_to_string(workspace.join("pnpm-workspace.yaml")).expect("workspace yaml");
    assert!(!workspace_yaml.contains("patchedDependencies:"), "workspace yaml: {workspace_yaml}");
    assert!(
        !workspace.join("patches/is-positive@1.0.0.patch").exists(),
        "patch file should be removed",
    );
    let installed =
        fs::read_to_string(workspace.join("node_modules/is-positive/index.js")).unwrap();
    assert!(!installed.contains("// patched"), "installed: {installed}");

    drop((root, mock_instance));
}

#[test]
fn patch_remove_keeps_missing_patch_files_as_noop_targets() {
    let (root, workspace, npmrc_info) =
        setup_configured_patch("is-positive@1.0.0", "is-positive@1.0.0.patch");
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    fs::remove_file(workspace.join("patches/is-positive@1.0.0.patch")).expect("remove patch file");

    pacquet(&workspace, ["patch-remove", "is-positive@1.0.0", "--reporter=silent"])
        .assert()
        .success();

    let workspace_yaml =
        fs::read_to_string(workspace.join("pnpm-workspace.yaml")).expect("workspace yaml");
    assert!(!workspace_yaml.contains("patchedDependencies:"), "workspace yaml: {workspace_yaml}");
    let installed =
        fs::read_to_string(workspace.join("node_modules/is-positive/index.js")).unwrap();
    assert!(!installed.contains("// patched"), "installed: {installed}");

    drop((root, mock_instance));
}

#[test]
fn patch_remove_errors_when_requested_patch_is_missing_from_manifest() {
    let (root, workspace, npmrc_info) =
        setup_configured_patch("is-positive@1.0.0", "is-positive@1.0.0.patch");
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let output = pacquet(&workspace, ["patch-remove", "is-negative", "--reporter=silent"])
        .output()
        .expect("run patch-remove");

    assert!(!output.status.success(), "unknown patch should fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("ERR_PNPM_PATCH_NOT_FOUND"), "stderr: {stderr}");
    assert!(
        workspace.join("patches/is-positive@1.0.0.patch").exists(),
        "existing patch should not be removed",
    );

    drop((root, mock_instance));
}

#[test]
fn patch_remove_errors_when_no_patches_are_configured() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "dependencies": {
                "is-positive": "1.0.0",
            },
        })
        .to_string(),
    )
    .expect("write package.json");

    let output = pacquet(&workspace, ["patch-remove", "--reporter=silent"])
        .output()
        .expect("run patch-remove");

    assert!(!output.status.success(), "patch-remove with no configured patches should fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("ERR_PNPM_NO_PATCHES_TO_REMOVE"), "stderr: {stderr}");

    drop((root, mock_instance));
}

#[test]
fn patch_remove_rejects_traversal_before_deleting_any_patch() {
    let (root, workspace, npmrc_info) =
        setup_patch_remove_project(&[("good", "patches/good.patch"), ("bad", "../outside.patch")]);
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    fs::create_dir_all(workspace.join("patches")).expect("create patches dir");
    fs::write(workspace.join("patches/good.patch"), "good patch").expect("write good patch");
    fs::write(root.path().join("outside.patch"), "outside patch").expect("write outside patch");

    let output = pacquet(&workspace, ["patch-remove", "good", "bad", "--reporter=silent"])
        .output()
        .expect("run patch-remove");

    assert!(!output.status.success(), "outside patch should fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("ERR_PNPM_PATCH_FILE_OUTSIDE_PATCHES_DIR"), "stderr: {stderr}");
    assert!(workspace.join("patches/good.patch").exists(), "good patch must remain");
    assert!(root.path().join("outside.patch").exists(), "outside patch must remain");

    drop((root, mock_instance));
}

#[test]
fn patch_remove_rejects_directory_entries_before_deleting_any_patch() {
    let (root, workspace, npmrc_info) = setup_patch_remove_project(&[
        ("good", "patches/good.patch"),
        ("bad", "patches/not-a-file.patch"),
    ]);
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    fs::create_dir_all(workspace.join("patches/not-a-file.patch")).expect("create patch directory");
    fs::write(workspace.join("patches/good.patch"), "good patch").expect("write good patch");

    let output = pacquet(&workspace, ["patch-remove", "good", "bad", "--reporter=silent"])
        .output()
        .expect("run patch-remove");

    assert!(!output.status.success(), "directory patch target should fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("ERR_PNPM_PATCH_FILE_IS_DIRECTORY"), "stderr: {stderr}");
    assert!(workspace.join("patches/good.patch").exists(), "good patch must remain");

    drop((root, mock_instance));
}

#[cfg(unix)]
#[test]
fn patch_remove_rejects_parent_symlink_outside_patches_dir_before_unlinking_target() {
    let (root, workspace, npmrc_info) =
        setup_patch_remove_project(&[("bad", "patches/linked-dir/dangling.patch")]);
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    let patches_dir = workspace.join("patches");
    let outside_dir = root.path().join("outside");
    let outside_link = outside_dir.join("dangling.patch");
    fs::create_dir_all(&patches_dir).expect("create patches dir");
    fs::create_dir_all(&outside_dir).expect("create outside dir");
    std::os::unix::fs::symlink(&outside_dir, patches_dir.join("linked-dir"))
        .expect("symlink parent dir");
    std::os::unix::fs::symlink(root.path().join("missing-target.patch"), &outside_link)
        .expect("symlink dangling target");

    let output = pacquet(&workspace, ["patch-remove", "bad", "--reporter=silent"])
        .output()
        .expect("run patch-remove");

    assert!(!output.status.success(), "parent symlink outside patches dir should fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("ERR_PNPM_PATCH_FILE_OUTSIDE_PATCHES_DIR"), "stderr: {stderr}");
    assert!(
        fs::symlink_metadata(&outside_link).expect("outside link").file_type().is_symlink(),
        "outside symlink target must remain",
    );

    drop((root, mock_instance));
}

#[cfg(unix)]
#[test]
fn patch_remove_unlinks_final_symlink_without_touching_target() {
    let (root, workspace, npmrc_info) =
        setup_patch_remove_project(&[("is-positive@1.0.0", "patches/linked.patch")]);
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    let patches_dir = workspace.join("patches");
    let outside_target = root.path().join("outside-target.patch");
    let patch_link = patches_dir.join("linked.patch");
    fs::create_dir_all(&patches_dir).expect("create patches dir");
    fs::write(&outside_target, "outside target").expect("write outside target");
    std::os::unix::fs::symlink(&outside_target, &patch_link).expect("symlink patch file");

    pacquet(&workspace, ["patch-remove", "is-positive@1.0.0", "--reporter=silent"])
        .assert()
        .success();

    assert!(!patch_link.exists(), "patch symlink should be removed");
    assert_eq!(fs::read_to_string(&outside_target).expect("read outside target"), "outside target");

    drop((root, mock_instance));
}
