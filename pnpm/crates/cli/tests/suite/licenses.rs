use crate::_utils;

use _utils::{enable_gvs_in_workspace_yaml, pacquet_in};
use assert_cmd::prelude::*;
use pnpm_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use serde_json::{Value, json};
use std::{fs, path::Path};

#[test]
fn licenses_normalizes_metadata_and_orders_groups_by_package() {
    let workspace = tempfile::tempdir().expect("create workspace");
    fs::write(
        workspace.path().join("package.json"),
        json!({
            "dependencies": {
                "a-b": "1.0.0",
                "a_b": "1.0.0",
                "alpha": "1.0.0",
                "zeta": "1.0.0",
            },
        })
        .to_string(),
    )
    .expect("write package.json");
    fs::write(
        workspace.path().join("pnpm-lock.yaml"),
        r"
lockfileVersion: '9.0'
importers:
  .:
    dependencies:
      a-b:
        specifier: 1.0.0
        version: 1.0.0
      a_b:
        specifier: 1.0.0
        version: 1.0.0
      alpha:
        specifier: 1.0.0
        version: 1.0.0
      zeta:
        specifier: 1.0.0
        version: 1.0.0
packages:
  a-b@1.0.0:
    resolution: {integrity: sha512-a-b}
  a_b@1.0.0:
    resolution: {integrity: sha512-a_b}
  alpha@1.0.0:
    resolution: {integrity: sha512-alpha}
  zeta@1.0.0:
    resolution: {integrity: sha512-zeta}
snapshots:
  a-b@1.0.0: {}
  a_b@1.0.0: {}
  alpha@1.0.0: {}
  zeta@1.0.0: {}
",
    )
    .expect("write lockfile");
    let virtual_store = workspace.path().join("node_modules/.pnpm");
    let a_dash_b_dir = virtual_store.join("a-b@1.0.0/node_modules/a-b");
    let a_underscore_b_dir = virtual_store.join("a_b@1.0.0/node_modules/a_b");
    let alpha_dir = virtual_store.join("alpha@1.0.0/node_modules/alpha");
    let zeta_dir = virtual_store.join("zeta@1.0.0/node_modules/zeta");
    fs::create_dir_all(&a_dash_b_dir).expect("create a-b directory");
    fs::create_dir_all(&a_underscore_b_dir).expect("create a_b directory");
    fs::create_dir_all(&alpha_dir).expect("create alpha directory");
    fs::create_dir_all(&zeta_dir).expect("create zeta directory");
    for (directory, name) in [(&a_dash_b_dir, "a-b"), (&a_underscore_b_dir, "a_b")] {
        fs::write(
            directory.join("package.json"),
            json!({
                "name": name,
                "version": "1.0.0",
                "license": "MIT",
            })
            .to_string(),
        )
        .expect("write collation fixture manifest");
    }
    fs::write(
        alpha_dir.join("package.json"),
        json!({
            "name": "alpha",
            "version": "1.0.0",
            "license": "Zlib",
            "author": "Alpha Team <alpha@example.com> (https://example.com/team)",
            "repository": "github:example/alpha",
        })
        .to_string(),
    )
    .expect("write alpha manifest");
    fs::write(
        zeta_dir.join("package.json"),
        json!({
            "name": "zeta",
            "version": "1.0.0",
            "license": "MIT",
        })
        .to_string(),
    )
    .expect("write zeta manifest");

    let output = pacquet_in(workspace.path())
        .args(["licenses", "list", "--json"])
        .output()
        .expect("run licenses");
    assert!(
        output.status.success(),
        "licenses should succeed: {}",
        String::from_utf8_lossy(&output.stderr),
    );

    let report: Value = serde_json::from_slice(&output.stdout).expect("parse licenses JSON");
    assert_eq!(report.as_object().unwrap().keys().collect::<Vec<_>>(), ["MIT", "Zlib"]);
    assert_eq!(report["Zlib"][0]["author"], "Alpha Team");
    assert_eq!(report["Zlib"][0]["homepage"], "https://github.com/example/alpha#readme");

    let output =
        pacquet_in(workspace.path()).args(["licenses", "list"]).output().expect("run licenses");
    assert!(
        output.status.success(),
        "licenses should succeed: {}",
        String::from_utf8_lossy(&output.stderr),
    );
    let table = String::from_utf8(output.stdout).expect("licenses table is UTF-8");
    assert!(
        table.find("a_b").expect("a_b row") < table.find("a-b").expect("a-b row"),
        "table should use JavaScript-compatible package collation:\n{table}",
    );
}

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
