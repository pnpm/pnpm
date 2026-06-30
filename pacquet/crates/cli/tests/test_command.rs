use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pacquet_testing_utils::bin::CommandTempCwd;
use serde_json::json;
use std::fs;

#[cfg_attr(target_os = "windows", ignore = "uses a POSIX shell command")]
#[test]
fn test_runs_declared_test_script() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let marker = workspace.join("tested.txt");
    let manifest = json!({
        "name": "test-command",
        "version": "0.0.0",
        "scripts": {
            "test": format!(r#"printf tested > "{}""#, marker.display()),
        },
    });
    fs::write(workspace.join("package.json"), manifest.to_string()).expect("write package.json");

    pacquet.with_arg("test").assert().success();

    assert_eq!(fs::read_to_string(marker).expect("read marker"), "tested");

    drop(root);
}
