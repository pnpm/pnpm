use super::pacquet_at;
use crate::_utils;
use assert_cmd::prelude::*;
use pnpm_testing_utils::bin::CommandTempCwd;
use std::fs;

#[test]
fn store_status_checks_global_virtual_store_packages() {
    check_global_virtual_store_status("");
}

#[test]
fn store_status_checks_custom_global_virtual_store_packages() {
    check_global_virtual_store_status("globalVirtualStoreDir: custom-links\n");
}

#[test]
fn store_status_checks_global_packages_with_virtual_store_dir_override() {
    check_global_virtual_store_status("virtualStoreDir: custom-links\n");
}

#[test]
fn store_status_uses_installed_build_approvals() {
    let CommandTempCwd {
        mut pacquet, workspace, root: _root, ..
    } = CommandTempCwd::init().add_mocked_registry();
    _utils::enable_gvs_in_workspace_yaml(&workspace, "");
    pacquet
        .args(["add", "@pnpm.e2e/generated-bins@1.0.0", "--allow-build=@pnpm.e2e/generated-bins"])
        .assert()
        .success();
    let settings = workspace.join("pnpm-workspace.yaml");
    let mut yaml: serde_json::Value =
        serde_saphyr::from_str(&fs::read_to_string(&settings).unwrap()).unwrap();
    yaml["allowBuilds"]["@pnpm.e2e/generated-bins"] = serde_json::Value::Bool(false);
    fs::write(&settings, serde_saphyr::to_string(&yaml).unwrap()).unwrap();
    pacquet_at(&workspace)
        .args(["store", "status"])
        .assert()
        .success();
}

#[test]
fn store_status_checks_global_packages_without_optional_dependencies() {
    let CommandTempCwd {
        mut pacquet, workspace, root: _root, ..
    } = CommandTempCwd::init().add_mocked_registry();
    _utils::enable_gvs_in_workspace_yaml(&workspace, "");
    pacquet
        .args(["add", "@pnpm.e2e/pkg-with-good-optional", "--no-optional"])
        .assert()
        .success();
    pacquet_at(&workspace)
        .args(["store", "status"])
        .assert()
        .success();
}

#[test]
fn store_status_reports_unverifiable_global_slots_after_node_version_changes() {
    let CommandTempCwd {
        mut pacquet, workspace, root: _root, ..
    } = CommandTempCwd::init().add_mocked_registry();
    _utils::enable_gvs_in_workspace_yaml(&workspace, "nodeVersion: 22.0.0\n");
    pacquet
        .args(["add", "@pnpm.e2e/generated-bins@1.0.0", "--allow-build=@pnpm.e2e/generated-bins"])
        .assert()
        .success();
    let settings = workspace.join("pnpm-workspace.yaml");
    let yaml = fs::read_to_string(&settings).unwrap();
    fs::write(&settings, yaml.replace("22.0.0", "24.0.0")).unwrap();
    let output = pacquet_at(&workspace)
        .args(["store", "status"])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success());
    assert!(stderr.contains("ERR_PNPM_STORE_STATUS_UNVERIFIABLE"), "{stderr}");
    assert!(!stderr.contains("ERR_PNPM_MODIFIED_DEPENDENCY"));
}

fn check_global_virtual_store_status(settings: &str) {
    let CommandTempCwd {
        mut pacquet, workspace, root: _root, ..
    } = CommandTempCwd::init().add_mocked_registry();
    _utils::enable_gvs_in_workspace_yaml(&workspace, settings);
    pacquet
        .args([
            "add",
            "is-odd@3.0.1",
            "@pnpm.e2e/abc@1.0.0",
            "@pnpm.e2e/generated-bins@1.0.0",
            "--allow-build=@pnpm.e2e/generated-bins",
        ])
        .assert()
        .success();

    let generated_bin = workspace.join("node_modules/@pnpm.e2e/generated-bins/bin/cmd1");
    assert!(generated_bin.is_file(), "build output at {generated_bin:?}");

    pacquet_at(&workspace)
        .args(["store", "status"])
        .assert()
        .success();

    fs::write(workspace.join("node_modules/@pnpm.e2e/abc/package.json"), "{}\n").unwrap();
    let output = pacquet_at(&workspace)
        .args(["store", "status"])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    eprintln!("stderr={stderr}");
    assert!(!output.status.success());
    assert!(stderr.contains("ERR_PNPM_MODIFIED_DEPENDENCY"));
    assert!(stderr.contains("@pnpm.e2e/abc@1.0.0("));
    assert!(!stderr.contains("is-odd@3.0.1"));
}
