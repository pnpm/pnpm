use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pacquet_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use std::{fs, process::Command};

#[test]
fn dedupe_writes_lockfile() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let manifest_path = workspace.join("package.json");
    fs::write(
        &manifest_path,
        serde_json::json!({
            "dependencies": {
                "@pnpm.e2e/pkg-with-1-dep": "100.0.0",
            },
        })
        .to_string(),
    )
    .expect("write package.json");

    let lockfile_path = workspace.join("pnpm-lock.yaml");
    pacquet.with_arg("dedupe").assert().success();

    assert!(lockfile_path.exists(), "dedupe must create pnpm-lock.yaml");
    let lockfile = fs::read_to_string(&lockfile_path).expect("read pnpm-lock.yaml");
    assert!(
        lockfile.contains("@pnpm.e2e/pkg-with-1-dep"),
        "lockfile must record the dependency:\n{lockfile}",
    );

    drop((root, mock_instance));
}

#[test]
fn dedupe_check_does_not_materialize_nor_write_lockfile() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let manifest_path = workspace.join("package.json");
    fs::write(
        &manifest_path,
        serde_json::json!({
            "dependencies": {
                "@pnpm.e2e/pkg-with-1-dep": "100.0.0",
            },
        })
        .to_string(),
    )
    .expect("write package.json");

    // Create a lockfile first by running dedupe
    pacquet.with_arg("dedupe").assert().success();

    // Recreate a pacquet command for the --check invocation
    let pacquet_check =
        Command::cargo_bin("pnpm").expect("find the pacquet binary").with_current_dir(&workspace);

    let lockfile_path = workspace.join("pnpm-lock.yaml");
    assert!(lockfile_path.exists(), "dedupe must create pnpm-lock.yaml");
    let lockfile_before = fs::read_to_string(&lockfile_path).expect("read pnpm-lock.yaml");

    pacquet_check.with_args(["dedupe", "--check"]).assert().success();

    assert!(
        !workspace.join("node_modules").exists(),
        "dedupe --check must not create node_modules",
    );
    let lockfile_after = fs::read_to_string(&lockfile_path).expect("read pnpm-lock.yaml");
    assert_eq!(lockfile_before, lockfile_after, "dedupe --check must not modify pnpm-lock.yaml");

    drop((root, mock_instance));
}
