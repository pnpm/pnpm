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
