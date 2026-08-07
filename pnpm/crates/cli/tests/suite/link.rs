use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pacquet_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use std::fs;

#[test]
fn link_fails_without_paths() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({ "name": "test-project", "version": "1.0.0" }).to_string(),
    )
    .expect("write package.json");

    let output = pacquet.with_arg("link").output().expect("spawn pacquet link");
    assert!(!output.status.success(), "link without paths must fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("You must provide a parameter"),
        "stderr should contain error message: {stderr}",
    );

    drop((root, mock_instance));
}

#[test]
fn ln_alias_fails_without_paths() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({ "name": "test-project", "version": "1.0.0" }).to_string(),
    )
    .expect("write package.json");

    let output = pacquet.with_arg("ln").output().expect("spawn pacquet ln");
    assert!(!output.status.success(), "ln alias without paths must fail");

    drop((root, mock_instance));
}

#[test]
fn link_fails_with_nonexistent_target() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({ "name": "test-project", "version": "1.0.0" }).to_string(),
    )
    .expect("write package.json");

    let output = pacquet
        .with_arg("link")
        .with_arg(workspace.join("definitely-missing-target").to_string_lossy().as_ref())
        .output()
        .expect("spawn pacquet link");
    assert!(!output.status.success(), "link to nonexistent path must fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("No package.json found"),
        "stderr should contain error message: {stderr}",
    );

    drop((root, mock_instance));
}

#[test]
fn link_succeeds_with_valid_target() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({ "name": "test-project", "version": "1.0.0" }).to_string(),
    )
    .expect("write package.json");

    let target_dir = root.path().join("target-project");
    fs::create_dir_all(&target_dir).expect("create target dir");
    fs::write(
        target_dir.join("package.json"),
        serde_json::json!({ "name": "target-project", "version": "1.0.0" }).to_string(),
    )
    .expect("write target package.json");

    pacquet.with_arg("link").with_arg("../target-project").assert().success();

    let manifest =
        pacquet_package_manifest::PackageManifest::from_path(workspace.join("package.json"))
            .expect("read manifest");
    let deps = manifest.value()["dependencies"].as_object().expect("dependencies exist");
    assert!(deps.contains_key("target-project"), "dependency must exist");
    assert_eq!(deps["target-project"], "link:../target-project");

    drop((root, mock_instance));
}

#[test]
fn link_succeeds_with_absolute_path() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({ "name": "test-project", "version": "1.0.0" }).to_string(),
    )
    .expect("write package.json");

    let target_dir = root.path().join("abs-target");
    fs::create_dir_all(&target_dir).expect("create target dir");
    fs::write(
        target_dir.join("package.json"),
        serde_json::json!({ "name": "abs-target", "version": "1.0.0" }).to_string(),
    )
    .expect("write target package.json");

    pacquet.with_arg("link").with_arg(target_dir.to_string_lossy().as_ref()).assert().success();

    let manifest =
        pacquet_package_manifest::PackageManifest::from_path(workspace.join("package.json"))
            .expect("read manifest");
    let deps = manifest.value()["dependencies"].as_object().expect("dependencies exist");
    assert!(deps.contains_key("abs-target"), "dependency must exist");

    drop((root, mock_instance));
}

#[test]
fn link_succeeds_with_multiple_targets() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({ "name": "test-project", "version": "1.0.0" }).to_string(),
    )
    .expect("write package.json");

    let target_a = root.path().join("multi-a");
    fs::create_dir_all(&target_a).expect("create target dir");
    fs::write(
        target_a.join("package.json"),
        serde_json::json!({ "name": "multi-a", "version": "1.0.0" }).to_string(),
    )
    .expect("write target package.json");

    let target_b = root.path().join("multi-b");
    fs::create_dir_all(&target_b).expect("create target dir");
    fs::write(
        target_b.join("package.json"),
        serde_json::json!({ "name": "multi-b", "version": "1.0.0" }).to_string(),
    )
    .expect("write target package.json");

    pacquet.with_arg("link").with_arg("../multi-a").with_arg("../multi-b").assert().success();

    let manifest =
        pacquet_package_manifest::PackageManifest::from_path(workspace.join("package.json"))
            .expect("read manifest");
    let deps = manifest.value()["dependencies"].as_object().expect("dependencies exist");
    assert!(deps.contains_key("multi-a"), "dependency multi-a must exist");
    assert!(deps.contains_key("multi-b"), "dependency multi-b must exist");

    drop((root, mock_instance));
}

#[test]
fn link_fails_target_no_name() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({ "name": "test-project", "version": "1.0.0" }).to_string(),
    )
    .expect("write package.json");

    let target_dir = root.path().join("target-project-no-name");
    fs::create_dir_all(&target_dir).expect("create target dir");
    fs::write(
        target_dir.join("package.json"),
        serde_json::json!({ "version": "1.0.0" }).to_string(),
    )
    .expect("write target package.json");

    let output =
        pacquet.with_arg("link").with_arg("../target-project-no-name").output().expect("spawn");
    assert!(!output.status.success(), "link to target without name must fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("does not have a name"),
        "stderr should contain error message: {stderr}",
    );

    drop((root, mock_instance));
}

#[test]
fn ln_alias_succeeds_with_valid_target() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({ "name": "test-project", "version": "1.0.0" }).to_string(),
    )
    .expect("write package.json");

    let target_dir = root.path().join("ln-target");
    fs::create_dir_all(&target_dir).expect("create target dir");
    fs::write(
        target_dir.join("package.json"),
        serde_json::json!({ "name": "ln-target", "version": "1.0.0" }).to_string(),
    )
    .expect("write target package.json");

    pacquet.with_arg("ln").with_arg("../ln-target").assert().success();

    let manifest =
        pacquet_package_manifest::PackageManifest::from_path(workspace.join("package.json"))
            .expect("read manifest");
    let deps = manifest.value()["dependencies"].as_object().expect("dependencies exist");
    assert!(deps.contains_key("ln-target"), "dependency must exist");
    assert_eq!(deps["ln-target"], "link:../ln-target");

    drop((root, mock_instance));
}

/// `link` records the `link:` specifier in `pnpm-workspace.yaml`'s
/// `overrides:` block (mirroring pnpm), not just in `dependencies`.
#[test]
fn link_persists_override_to_workspace_yaml() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({ "name": "test-project", "version": "1.0.0" }).to_string(),
    )
    .expect("write package.json");

    let target_dir = root.path().join("override-target");
    fs::create_dir_all(&target_dir).expect("create target dir");
    fs::write(
        target_dir.join("package.json"),
        serde_json::json!({ "name": "override-target", "version": "1.0.0" }).to_string(),
    )
    .expect("write target package.json");

    pacquet.with_arg("link").with_arg("../override-target").assert().success();

    let workspace_yaml =
        fs::read_to_string(workspace.join("pnpm-workspace.yaml")).expect("read workspace yaml");
    assert!(workspace_yaml.contains("overrides:"), "overrides block must exist: {workspace_yaml}");
    assert!(
        workspace_yaml.contains("override-target: link:../override-target"),
        "override must record the link spec: {workspace_yaml}",
    );

    drop((root, mock_instance));
}

/// When the package is already declared in another dependency field, `link`
/// records the override but does not add a duplicate `dependencies` entry,
/// matching pnpm's `DEPENDENCIES_FIELDS.every(...)` guard. The override is
/// what makes the `link:` spec win, so the install links the local target
/// even though the existing spec is a registry range.
#[test]
fn link_existing_dependency_writes_override_only() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "name": "test-project",
            "version": "1.0.0",
            "devDependencies": { "target-project": "^1.0.0" },
        })
        .to_string(),
    )
    .expect("write package.json");

    let target_dir = root.path().join("target-project");
    fs::create_dir_all(&target_dir).expect("create target dir");
    fs::write(
        target_dir.join("package.json"),
        serde_json::json!({ "name": "target-project", "version": "1.0.0" }).to_string(),
    )
    .expect("write target package.json");

    pacquet.with_arg("link").with_arg("../target-project").assert().success();

    let manifest =
        pacquet_package_manifest::PackageManifest::from_path(workspace.join("package.json"))
            .expect("read manifest");
    assert!(
        manifest.value().get("dependencies").is_none(),
        "no dependencies entry should be added when already declared elsewhere",
    );
    let dev_deps = manifest.value()["devDependencies"].as_object().expect("devDependencies exist");
    assert_eq!(dev_deps["target-project"], "^1.0.0", "the existing entry stays untouched");

    let workspace_yaml =
        fs::read_to_string(workspace.join("pnpm-workspace.yaml")).expect("read workspace yaml");
    assert!(
        workspace_yaml.contains("target-project: link:../target-project"),
        "override must record the link spec: {workspace_yaml}",
    );

    // The install succeeded despite the registry-spec devDependency, which is
    // only possible because the in-memory override rewrote it to the local
    // `link:` — confirm the symlink was created rather than a registry fetch.
    let linked = workspace.join("node_modules").join("target-project");
    assert!(
        fs::symlink_metadata(&linked).is_ok_and(|meta| meta.file_type().is_symlink()),
        "node_modules/target-project must be a symlink created from the link override",
    );

    drop((root, mock_instance));
}
