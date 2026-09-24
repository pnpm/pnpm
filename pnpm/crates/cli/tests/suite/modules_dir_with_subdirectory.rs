//! A `modulesDir` of several path segments, such as `www/modules`, is
//! joined whole onto every project's directory, as pnpm 11 does.

use crate::_utils::{append_workspace_yaml_key, pacquet_in};
use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use pretty_assertions::assert_eq;
use std::fs;

#[test]
fn every_install_puts_a_members_dependencies_in_its_own_nested_modules_dir() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    append_workspace_yaml_key(&workspace, "modulesDir", "www/modules");
    append_workspace_yaml_key(&workspace, "packages", "['packages/*']");
    fs::write(workspace.join("package.json"), r#"{"name":"root","private":true}"#)
        .expect("write package.json");
    let member = workspace.join("packages/member");
    fs::create_dir_all(&member).expect("create member");
    fs::write(
        member.join("package.json"),
        r#"{"name":"member","version":"1.0.0","dependencies":{"is-positive":"1.0.0"}}"#,
    )
    .expect("write member package.json");
    let assert_member_layout = |install: &str| {
        assert!(
            member.join("www/modules/is-positive/package.json").exists(),
            "{install}: is-positive is missing from packages/member/www/modules",
        );
        assert!(!member.join("modules").exists(), "{install}: stray packages/member/modules");
        assert!(!workspace.join("www/packages").exists(), "{install}: stray www/packages");
        assert!(workspace.join("www/modules/.pnpm").is_dir(), "{install}: no virtual store");
    };

    pacquet_in(&workspace)
        .with_arg("install")
        .assert()
        .success();
    assert_member_layout("install");

    fs::remove_dir_all(workspace.join("www")).expect("remove www");
    fs::remove_dir_all(member.join("www")).expect("remove member www");
    pacquet_in(&workspace)
        .with_args(["install", "--frozen-lockfile"])
        .assert()
        .success();
    assert_member_layout("install --frozen-lockfile");

    drop((root, mock_instance));
}

#[test]
fn bin_prints_the_whole_nested_modules_dir() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    fs::write(workspace.join("pnpm-workspace.yaml"), "modulesDir: www/modules\n")
        .expect("write pnpm-workspace.yaml");

    let output = pacquet_in(&workspace)
        .with_arg("bin")
        .output()
        .expect("run pnpm bin");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));

    let workspace = dunce::canonicalize(&workspace).expect("canonicalize workspace");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        format!(
            "{}\n",
            workspace
                .join("www/modules")
                .join(".bin")
                .display()
        ),
    );

    drop(root);
}
