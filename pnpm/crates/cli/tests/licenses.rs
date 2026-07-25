pub mod _utils;

use _utils::{enable_gvs_in_workspace_yaml, pacquet_in};
use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pacquet_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use serde_json::{Value, json};
use std::{fs, path::Path};

#[test]
fn licenses_reads_package_metadata_from_the_global_virtual_store() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    enable_gvs_in_workspace_yaml(&workspace, "");
    fs::write(
        workspace.join("package.json"),
        json!({
            "dependencies": {
                "@pnpm.e2e/has-different-licenses": "1.0.0",
            },
        })
        .to_string(),
    )
    .expect("write package.json");

    pacquet.with_arg("install").assert().success();

    for subcommand in ["list", "ls"] {
        let output = pacquet_in(&workspace)
            .with_args(["licenses", subcommand, "--json"])
            .output()
            .expect("spawn pacquet licenses");
        assert!(
            output.status.success(),
            "licenses {subcommand} should succeed: {}",
            String::from_utf8_lossy(&output.stderr),
        );

        let licenses: Value = serde_json::from_slice(&output.stdout).expect("parse licenses JSON");
        let packages = licenses["MIT"].as_array().expect("MIT license group");
        assert_eq!(packages.len(), 1);
        assert_eq!(packages[0]["name"], "@pnpm.e2e/has-different-licenses");
        assert_eq!(packages[0]["versions"], json!(["1.0.0"]));
        assert!(
            packages[0]["paths"]
                .as_array()
                .expect("package paths")
                .iter()
                .all(|path| Path::new(path.as_str().expect("path string")).exists()),
            "reported package paths should exist: {}",
            packages[0]["paths"],
        );
    }

    drop((root, mock_instance));
}
