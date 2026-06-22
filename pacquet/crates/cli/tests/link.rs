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
        .with_arg("/nonexistent/path")
        .output()
        .expect("spawn pacquet link");
    assert!(!output.status.success(), "link to nonexistent path must fail");

    drop((root, mock_instance));
}
