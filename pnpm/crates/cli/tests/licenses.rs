pub mod _utils;

use _utils::{enable_gvs_in_workspace_yaml, pacquet_in};
use assert_cmd::prelude::*;
use pacquet_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use serde_json::{Value, json};
use std::{fs, path::Path};

#[test]
fn licenses_reads_global_store_metadata_with_a_manifest_selected_runtime() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    let config_home = root.path().join("config");
    fs::create_dir(&config_home).expect("create empty config home");

    enable_gvs_in_workspace_yaml(
        &workspace,
        "allowBuilds:\n  '@pnpm.e2e/install-script-example': true\n",
    );
    fs::write(
        workspace.join("package.json"),
        json!({
            "devEngines": {
                "runtime": {
                    "name": "node",
                    "version": "1",
                    "onFail": "ignore",
                },
            },
            "dependencies": {
                // This engine-constrained dependency makes installation resolve the GVS engine
                // from the manifest-selected runtime instead of deferring to the host Node.
                "@pnpm.e2e/for-legacy-node": "1.0.0",
                "@pnpm.e2e/install-script-example": "1.0.0",
                "@pnpm.e2e/legacy-license": "1.0.0",
            },
        })
        .to_string(),
    )
    .expect("write package.json");

    let mut pacquet = pacquet;
    pacquet.env("XDG_CONFIG_HOME", &config_home).arg("install").assert().success();

    for subcommand in ["list", "ls"] {
        let mut licenses_command = pacquet_in(&workspace);
        let output = licenses_command
            .env("XDG_CONFIG_HOME", &config_home)
            .args(["licenses", subcommand, "--json"])
            .output()
            .expect("spawn pacquet licenses");
        assert!(
            output.status.success(),
            "licenses {subcommand} should succeed: {}",
            String::from_utf8_lossy(&output.stderr),
        );

        let licenses: Value = serde_json::from_slice(&output.stdout).expect("parse licenses JSON");
        let packages = licenses["MIT"].as_array().expect("MIT license group");
        assert_eq!(
            packages
                .iter()
                .map(|package| package["name"].as_str().expect("package name"))
                .collect::<Vec<_>>(),
            [
                "@pnpm.e2e/for-legacy-node",
                "@pnpm.e2e/install-script-example",
                "@pnpm.e2e/legacy-license",
            ],
        );
        for package in packages {
            assert_eq!(package["versions"], json!(["1.0.0"]));
            assert!(
                package["paths"]
                    .as_array()
                    .expect("package paths")
                    .iter()
                    .all(|path| Path::new(path.as_str().expect("path string")).exists()),
                "reported package paths should exist: {}",
                package["paths"],
            );
        }
    }

    drop((root, mock_instance));
}
