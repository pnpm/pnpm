pub mod _utils;

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pacquet_testing_utils::bin::CommandTempCwd;

#[test]
fn install_test() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();

    // Create a package.json with a test script and a dependency
    std::fs::write(
        workspace.join("package.json"),
        r#"{
            "name": "test-install-test",
            "scripts": {
                "test": "node -e \"console.log('test ran successfully');\""
            },
            "dependencies": {
                "@pnpm.e2e/hello-world-js-bin": "1.0.0"
            }
        }"#,
    )
    .unwrap();

    let output = pacquet.with_args(["install-test"]).assert().success().get_output().clone();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("test ran successfully"), "stdout: {stdout}");

    let is_positive_dir =
        workspace.join("node_modules").join("@pnpm.e2e").join("hello-world-js-bin");
    assert!(is_positive_dir.exists(), "dependency not installed");
    drop((root, npmrc_info));
}
