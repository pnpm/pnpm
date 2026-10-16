use assert_cmd::prelude::*;
use pnpm_testing_utils::bin::CommandTempCwd;
use std::fs;

fn set_max_unpacked_size(workspace: &std::path::Path, bytes: u64) {
    let workspace_yaml = workspace.join("pnpm-workspace.yaml");
    let mut yaml = fs::read_to_string(&workspace_yaml).expect("read pnpm-workspace.yaml");
    yaml.push_str(&format!("maxUnpackedSize: {bytes}\n"));
    fs::write(&workspace_yaml, yaml).expect("write pnpm-workspace.yaml");
}

#[test]
fn install_fails_when_a_package_unpacks_past_max_unpacked_size() {
    let CommandTempCwd { mut pacquet, workspace, root: _root, .. } =
        CommandTempCwd::init().add_mocked_registry();
    set_max_unpacked_size(&workspace, 1);

    let output = pacquet
        .arg("add")
        .arg("is-odd@3.0.1")
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&output.get_output().stderr);
    assert!(stderr.contains("ERR_PNPM_TARBALL_UNPACKED_SIZE_EXCEEDED"), "{stderr}");
    assert!(stderr.contains("Tarball unpacks to more than 1 bytes"), "{stderr}");
    assert!(stderr.contains("maxUnpackedSize"), "{stderr}");
    assert!(!workspace.join("node_modules/is-odd/package.json").exists());
}

#[test]
fn install_succeeds_when_max_unpacked_size_covers_the_package() {
    let CommandTempCwd { mut pacquet, workspace, root: _root, .. } =
        CommandTempCwd::init().add_mocked_registry();
    set_max_unpacked_size(&workspace, 1024 * 1024);

    pacquet
        .arg("add")
        .arg("is-odd@3.0.1")
        .assert()
        .success();
    assert!(workspace.join("node_modules/is-odd/package.json").exists());
}
