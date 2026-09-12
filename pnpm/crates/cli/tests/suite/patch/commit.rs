use super::{
    AddMockedRegistry, fs, pacquet, setup_installed, setup_installed_workspace_project,
    write_patch_edit,
};
use assert_cmd::assert::OutputAssertExt;

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
fn patch_commit_writes_an_applicable_patch_for_a_deleted_file() {
    let (root, workspace, npmrc_info) = setup_installed();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    pacquet(&workspace, ["patch", "is-positive@1.0.0", "--reporter=silent"]).assert().success();
    let edit_dir = workspace.join("node_modules/.pnpm_patches/is-positive@1.0.0");
    fs::remove_file(edit_dir.join("readme.md")).expect("delete readme.md");

    pacquet(
        &workspace,
        ["patch-commit", edit_dir.to_str().expect("utf8 edit dir"), "--reporter=silent"],
    )
    .assert()
    .success();

    let patch =
        fs::read_to_string(workspace.join("patches/is-positive@1.0.0.patch")).expect("patch file");
    eprintln!("PATCH:\n{patch}");
    assert!(patch.contains("diff --git a/readme.md b/readme.md\n"), "patch: {patch}");
    assert!(
        !workspace.join("node_modules/is-positive/readme.md").exists(),
        "the reinstall should have dropped readme.md",
    );

    // Re-running `patch` applies the committed patch to a fresh copy of the package, so it fails
    // when the generated patch cannot be parsed or applied.
    fs::remove_dir_all(&edit_dir).expect("remove edit dir");
    pacquet(&workspace, ["patch", "is-positive@1.0.0", "--reporter=silent"]).assert().success();

    assert!(!edit_dir.join("readme.md").exists(), "readme.md should stay deleted");

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
