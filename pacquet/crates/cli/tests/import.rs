use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pacquet_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use std::fs;

#[test]
fn import_creates_lockfile_from_scratch() {
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
    assert!(!lockfile_path.exists(), "lockfile must not exist before import");

    pacquet.with_arg("import").assert().success();

    assert!(lockfile_path.exists(), "import must create pnpm-lock.yaml");
    let lockfile = fs::read_to_string(&lockfile_path).expect("read pnpm-lock.yaml");
    assert!(
        lockfile.contains("@pnpm.e2e/pkg-with-1-dep"),
        "lockfile must record the direct dependency:\n{lockfile}",
    );
    assert!(
        lockfile.contains("@pnpm.e2e/pkg-with-1-dep@100.0.0"),
        "lockfile must pin the resolved version:\n{lockfile}",
    );
    assert!(!workspace.join("node_modules").exists(), "import must not create node_modules");

    drop((root, mock_instance));
}

#[test]
fn import_replaces_existing_lockfile() {
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
    fs::write(&lockfile_path, "# stale placeholder lockfile\n").expect("write stale lockfile");

    pacquet.with_arg("import").assert().success();

    assert!(lockfile_path.exists(), "import must keep pnpm-lock.yaml");
    let lockfile = fs::read_to_string(&lockfile_path).expect("read pnpm-lock.yaml");
    assert!(
        !lockfile.contains("stale placeholder lockfile"),
        "import must replace the old lockfile content",
    );
    assert!(
        lockfile.contains("@pnpm.e2e/pkg-with-1-dep"),
        "import must write a valid lockfile:\n{lockfile}",
    );

    drop((root, mock_instance));
}
