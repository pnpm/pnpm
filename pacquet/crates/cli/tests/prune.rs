use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pacquet_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use std::fs;

#[test]
fn prune_writes_lockfile() {
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
    pacquet.with_arg("prune").assert().success();

    assert!(lockfile_path.exists(), "prune must create pnpm-lock.yaml");
    let lockfile = fs::read_to_string(&lockfile_path).expect("read pnpm-lock.yaml");
    assert!(
        lockfile.contains("@pnpm.e2e/pkg-with-1-dep"),
        "lockfile must record the dependency:\n{lockfile}",
    );

    drop((root, mock_instance));
}

#[test]
fn prune_with_prod_only_omits_dev_deps() {
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
            "devDependencies": {
                "@pnpm.e2e/hello-world-js-bin": "1.0.0",
            }
        })
        .to_string(),
    )
    .expect("write package.json");

    let lockfile_path = workspace.join("pnpm-lock.yaml");
    pacquet.with_args(["prune", "--prod"]).assert().success();

    assert!(lockfile_path.exists(), "prune --prod must create pnpm-lock.yaml");
    let lockfile = fs::read_to_string(&lockfile_path).expect("read pnpm-lock.yaml");
    assert!(
        lockfile.contains("@pnpm.e2e/pkg-with-1-dep"),
        "prune --prod must include prod dependencies:\n{lockfile}",
    );
    assert!(
        !lockfile.contains("@pnpm.e2e/hello-world-js-bin"),
        "prune --prod must NOT include dev dependencies:\n{lockfile}",
    );

    drop((root, mock_instance));
}
