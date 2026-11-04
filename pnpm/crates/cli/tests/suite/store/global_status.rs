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

#[test]
fn store_status_rejects_project_links_to_a_different_global_slot() {
    let CommandTempCwd {
        mut pacquet, workspace, root: _root, ..
    } = CommandTempCwd::init().add_mocked_registry();
    _utils::enable_gvs_in_workspace_yaml(&workspace, "");
    pacquet
        .args(["add", "is-odd@3.0.1"])
        .assert()
        .success();
    let replacement = workspace.join("replacement");
    fs::create_dir(&replacement).unwrap();
    fs::write(replacement.join("package.json"), "{}\n").unwrap();
    pnpm_fs::force_symlink_dir(&replacement, &workspace.join("node_modules/is-odd")).unwrap();
    let output = pacquet_at(&workspace)
        .args(["store", "status"])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success());
    assert!(stderr.contains("ERR_PNPM_STORE_STATUS_UNVERIFIABLE"), "{stderr}");
}

#[test]
fn store_status_reports_dangling_global_links_as_unverifiable() {
    let CommandTempCwd {
        mut pacquet, workspace, root: _root, ..
    } = CommandTempCwd::init().add_mocked_registry();
    _utils::enable_gvs_in_workspace_yaml(&workspace, "");
    pacquet
        .args(["add", "is-odd@3.0.1"])
        .assert()
        .success();
    let package_dir = fs::canonicalize(workspace.join("node_modules/is-odd")).unwrap();
    fs::remove_dir_all(package_dir).unwrap();
    let output = pacquet_at(&workspace)
        .args(["store", "status"])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success());
    assert!(stderr.contains("ERR_PNPM_STORE_STATUS_UNVERIFIABLE"), "{stderr}");
}

#[test]
fn store_status_checks_custom_modules_directory() {
    check_workspace_links("modulesDir: custom_modules\n", "custom_modules");
}

#[test]
fn store_status_checks_deduplicated_workspace_links() {
    check_workspace_links("dedupeDirectDeps: true\n", "node_modules");
}

#[test]
fn store_status_rejects_escaping_importer_paths() {
    let CommandTempCwd {
        mut pacquet, workspace, root: _root, ..
    } = CommandTempCwd::init().add_mocked_registry();
    _utils::enable_gvs_in_workspace_yaml(&workspace, "");
    pacquet
        .args(["add", "is-odd@3.0.1"])
        .assert()
        .success();
    let lockfile = workspace.join("node_modules/.pnpm/lock.yaml");
    let mut value: serde_json::Value =
        serde_saphyr::from_str(&fs::read_to_string(&lockfile).unwrap()).unwrap();
    let importers = value["importers"].as_object_mut().unwrap();
    let root = importers.remove(".").unwrap();
    importers.insert("../outside".to_string(), root);
    fs::write(&lockfile, serde_saphyr::to_string(&value).unwrap()).unwrap();
    let output = pacquet_at(&workspace)
        .args(["store", "status"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("ERR_PNPM_PACKAGE_MANAGER_UNSAFE_IMPORTER_PATH"), "{stderr}");
}

fn check_workspace_links(settings: &str, modules_dir: &str) {
    let CommandTempCwd {
        mut pacquet, workspace, root: _root, ..
    } = CommandTempCwd::init().add_mocked_registry();
    _utils::enable_gvs_in_workspace_yaml(
        &workspace,
        &format!("packages:\n  - packages/*\n{settings}"),
    );
    let manifest = r#"{"name":"root-project","dependencies":{"is-odd":"3.0.1"}}"#;
    fs::write(workspace.join("package.json"), manifest).unwrap();
    fs::create_dir_all(workspace.join("packages/child")).unwrap();
    fs::write(
        workspace.join("packages/child/package.json"),
        manifest.replace("root-project", "child-project"),
    )
    .unwrap();
    pacquet
        .arg("install")
        .assert()
        .success();
    assert!(
        workspace
            .join(modules_dir)
            .join("is-odd")
            .exists(),
    );
    pacquet_at(&workspace)
        .args(["store", "status"])
        .assert()
        .success();
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
