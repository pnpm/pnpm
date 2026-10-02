use super::{
    AddMockedRegistry, CommandExtra, CommandTempCwd, append_workspace_yaml_key, fs, pacquet_cmd,
    write_workspace,
};
use assert_cmd::assert::OutputAssertExt;

/// A workspace dependency deployed from a shared lockfile is a copy of its
/// source directory. `packageImportMethod: hardlink` makes every other
/// import link, so a deployed file sharing an inode with its source would
/// show the source edit below.
fn assert_deployed_workspace_dependency_is_a_copy(node_linker: &str) {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_workspace(&workspace, false);
    append_workspace_yaml_key(&workspace, "packageImportMethod", "hardlink");
    append_workspace_yaml_key(&workspace, "nodeLinker", node_linker);
    let source_file = workspace.join("packages/lib/index.js");
    fs::write(&source_file, "module.exports = 1").unwrap();

    pacquet
        .with_arg("install")
        .assert()
        .success();
    pacquet_cmd(&workspace)
        .with_args(["--filter", "app", "deploy", "--prod", "deploy"])
        .assert()
        .success();

    fs::write(&source_file, "module.exports = 2").unwrap();
    let deployed_file = workspace.join("deploy/node_modules/lib/index.js");
    assert_eq!(fs::read_to_string(deployed_file).unwrap(), "module.exports = 1");

    drop((root, mock_instance));
}

#[test]
fn shared_lockfile_deploy_copies_workspace_dependencies_with_the_isolated_linker() {
    assert_deployed_workspace_dependency_is_a_copy("isolated");
}

#[test]
fn shared_lockfile_deploy_copies_workspace_dependencies_with_the_hoisted_linker() {
    assert_deployed_workspace_dependency_is_a_copy("hoisted");
}
