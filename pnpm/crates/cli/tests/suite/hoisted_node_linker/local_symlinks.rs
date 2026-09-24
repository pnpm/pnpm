use super::{
    AddMockedRegistry, CommandExtra, CommandTempCwd, fs, write_manifest, write_workspace_yaml,
};
use assert_cmd::assert::OutputAssertExt;
use std::{os::unix::fs::symlink, path::Path};

/// A directory dependency keeps the symlinks inside it when the hoisted
/// linker copies it into `node_modules`. A hardlink of a symlink is a
/// symlink on Unix, so only the copy method shows a link imported as a
/// file.
#[test]
fn hoisted_install_preserves_internal_symlinks_of_a_directory_dependency() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let local = workspace.join("local-pkg");
    fs::create_dir_all(local.join("sub")).unwrap();
    fs::write(
        local.join("package.json"),
        serde_json::json!({ "name": "local-pkg", "version": "1.0.0" }).to_string(),
    )
    .unwrap();
    fs::write(local.join("real.txt"), "real").unwrap();
    fs::write(local.join("sub/nested.txt"), "nested").unwrap();
    symlink("real.txt", local.join("file-link")).unwrap();
    symlink("sub", local.join("dir-link")).unwrap();
    write_manifest(&workspace, serde_json::json!({ "local-pkg": "file:./local-pkg" }));
    write_workspace_yaml(&workspace, "nodeLinker: hoisted\npackageImportMethod: copy\n");

    pacquet
        .with_arg("install")
        .assert()
        .success();

    let installed = workspace.join("node_modules/local-pkg");
    assert_eq!(fs::read_link(installed.join("file-link")).unwrap(), Path::new("real.txt"));
    assert_eq!(fs::read_link(installed.join("dir-link")).unwrap(), Path::new("sub"));
    assert_eq!(fs::read_to_string(installed.join("dir-link/nested.txt")).unwrap(), "nested");

    drop((root, mock_instance));
}
