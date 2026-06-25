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
