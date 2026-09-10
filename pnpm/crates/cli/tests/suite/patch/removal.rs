use super::{
    AddMockedRegistry, CommandTempCwd, append_unused_filtered_patch, fs, pacquet,
    setup_configured_patch, setup_configured_patch_with_allow_unused,
    setup_configured_patch_with_yaml, setup_filtered_patch_workspace, setup_patch_remove_project,
};
use assert_cmd::assert::OutputAssertExt;

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

#[test]
fn unused_patch_fails_with_err_pnpm_unused_patch() {
    let (root, workspace, npmrc_info) = setup_configured_patch_with_allow_unused(
        &[
            ("is-positive@1.0.0", "is-positive@1.0.0.patch"),
            ("is-negative@1.0.0", "is-positive@1.0.0.patch"),
        ],
        false,
    );
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let output = pacquet(&workspace, ["install"]).output().expect("run install");

    assert!(!output.status.success(), "install with unused patch should fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("ERR_PNPM_UNUSED_PATCH"),
        "stderr should contain ERR_PNPM_UNUSED_PATCH: {stderr}",
    );
    assert!(
        stderr.contains("is-negative@1.0.0"),
        "stderr should mention the unused patch key: {stderr}",
    );

    drop((root, mock_instance));
}

#[test]
fn unused_patch_warns_when_allow_unused_patches_is_set() {
    let (root, workspace, npmrc_info) = setup_configured_patch_with_allow_unused(
        &[
            ("is-positive@1.0.0", "is-positive@1.0.0.patch"),
            ("is-negative@1.0.0", "is-positive@1.0.0.patch"),
        ],
        true,
    );
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let output = pacquet(&workspace, ["install"]).output().expect("run install");

    assert!(output.status.success(), "install should succeed with allowUnusedPatches");
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    assert!(
        combined.contains("not used"),
        "output should warn about unused patches: stdout={stdout}, stderr={stderr}",
        stdout = String::from_utf8_lossy(&output.stdout),
        stderr = String::from_utf8_lossy(&output.stderr),
    );
    assert!(
        combined.contains("is-negative@1.0.0"),
        "warning should mention the unused patch key: stdout={stdout}, stderr={stderr}",
        stdout = String::from_utf8_lossy(&output.stdout),
        stderr = String::from_utf8_lossy(&output.stderr),
    );

    drop((root, mock_instance));
}

#[test]
fn legacy_deploy_honors_allow_unused_patches_overrides() {
    let (root, workspace, npmrc_info) = setup_configured_patch_with_yaml(
        "is-positive@1.0.0",
        "is-positive@1.0.0.patch",
        "packages:\n  - 'packages/*'\n",
    );
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    let app = workspace.join("packages/app");
    fs::create_dir_all(&app).expect("create app directory");
    fs::write(
        app.join("package.json"),
        serde_json::json!({ "name": "app", "version": "1.0.0" }).to_string(),
    )
    .expect("write app manifest");

    pacquet(&workspace, ["install"]).assert().success();
    pacquet(
        &workspace,
        [
            "--config.allow-unused-patches=true",
            "--filter=app",
            "deploy",
            "--legacy",
            "config-deploy",
        ],
    )
    .assert()
    .success();
    pacquet(&workspace, ["--filter=app", "deploy", "--legacy", "env-deploy"])
        .env("PNPM_CONFIG_ALLOW_UNUSED_PATCHES", "true")
        .assert()
        .success();

    drop((root, mock_instance));
}

/// pnpm skips patch usage validation when a first filtered install's
/// previous wanted lockfile does not cover every resolved importer.
#[test]
fn unused_patch_is_not_checked_on_a_filtered_install() {
    let (root, workspace, npmrc_info) = setup_filtered_patch_workspace();
    append_unused_filtered_patch(&workspace);
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let output =
        pacquet(&workspace, ["install", "--filter", "pkg-a"]).output().expect("run install");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "filtered install should succeed: {stderr}");
    assert!(
        !stderr.contains("ERR_PNPM_UNUSED_PATCH"),
        "filtered install should not run the unused-patch check: {stderr}",
    );

    drop((root, mock_instance));
}

/// A filtered selection augmented with the workspace root covers every
/// importer once the previous wanted lockfile does too, so pnpm validates
/// unused patches.
#[test]
fn unused_patch_is_checked_for_a_complete_root_augmented_filtered_install() {
    let (root, workspace, npmrc_info) = setup_filtered_patch_workspace();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    pacquet(&workspace, ["install"]).assert().success();
    append_unused_filtered_patch(&workspace);

    let output =
        pacquet(&workspace, ["install", "--filter", "pkg-a"]).output().expect("run install");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "complete filtered install should fail: {stderr}");
    assert!(
        stderr.contains("ERR_PNPM_UNUSED_PATCH"),
        "complete filtered install should run the unused-patch check: {stderr}",
    );

    drop((root, mock_instance));
}
