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

    pacquet.with_arg("link").with_arg("../target-project-no-name").assert().failure();

    drop((root, mock_instance));
}
