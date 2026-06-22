use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pacquet_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use std::{fs, path::Path, process::Command};

fn pacquet_at(workspace: &Path) -> Command {
    Command::cargo_bin("pacquet").expect("find the pacquet binary").with_current_dir(workspace)
}

#[test]
fn fetch_requires_existing_lockfile() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "dependencies": { "@pnpm.e2e/pkg-with-1-dep": "100.0.0" },
        })
        .to_string(),
    )
    .expect("write package.json");

    let output = pacquet.with_arg("fetch").output().expect("spawn pacquet fetch");
    assert!(
        !output.status.success(),
        "fetch without lockfile must fail (stderr: {})",
        String::from_utf8_lossy(&output.stderr),
    );

    drop((root, mock_instance));
}

#[test]
fn fetch_succeeds_with_lockfile() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { store_dir, mock_instance, .. } = npmrc_info;

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "dependencies": { "@pnpm.e2e/pkg-with-1-dep": "100.0.0" },
        })
        .to_string(),
    )
    .expect("write package.json");

    pacquet.with_args(["install", "--lockfile-only"]).assert().success();
    assert!(workspace.join("pnpm-lock.yaml").exists(), "lockfile must exist after --lockfile-only");

    pacquet_at(&workspace).with_arg("fetch").assert().success();

    drop((root, mock_instance, store_dir));
}
