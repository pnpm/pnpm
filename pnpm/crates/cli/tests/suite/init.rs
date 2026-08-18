use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::{bin::CommandTempCwd, fs::get_filenames_in_folder};
use pretty_assertions::assert_eq;
use std::{fs, path::Path};

#[test]
fn should_create_package_json() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    pacquet.with_arg("init").assert().success();

    let manifest_path = workspace.join("package.json");
    dbg!(&manifest_path);

    eprintln!("Content of package.json");
    let package_json_content = fs::read_to_string(&manifest_path).expect("read from package.json");
    // The pinned version moves with every release, so the snapshot records
    // the shape of the pin rather than the number.
    insta::assert_snapshot!(
        package_json_content.replace(pnpm_config::PNPM_VERSION, "<pnpm-version>")
    );

    eprintln!("Created files");
    assert_eq!(get_filenames_in_folder(&workspace), ["package.json"]);

    drop(root);
}

#[test]
fn should_throw_on_existing_file() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();

    let manifest_path = workspace.join("package.json");
    dbg!(&manifest_path);

    eprintln!("Creating package.json...");
    fs::write(&manifest_path, "{}").expect("write to package.json");

    eprintln!("Executing pacquet init...");
    let output = pacquet.with_arg("init").output().expect("execute pacquet init");
    dbg!(&output);

    eprintln!("Exit status code");
    assert!(!output.status.success());

    eprintln!("Stderr");
    insta::assert_snapshot!(String::from_utf8_lossy(&output.stderr).trim_end());

    drop(root);
}

#[test]
fn no_init_package_manager_leaves_the_manifest_unpinned() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    pacquet.with_arg("init").with_arg("--no-init-package-manager").assert().success();

    assert_unpinned(&workspace);

    drop(root);
}

#[test]
fn init_package_manager_off_in_the_workspace_manifest_leaves_the_manifest_unpinned() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    fs::write(workspace.join("pnpm-workspace.yaml"), "initPackageManager: false\n")
        .expect("write to pnpm-workspace.yaml");
    pacquet.with_arg("init").assert().success();

    assert_unpinned(&workspace);

    drop(root);
}

#[test]
fn a_workspace_root_is_pinned() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    fs::write(workspace.join("pnpm-workspace.yaml"), "packages:\n  - packages/*\n")
        .expect("write to pnpm-workspace.yaml");
    pacquet.with_arg("init").assert().success();

    let manifest =
        fs::read_to_string(workspace.join("package.json")).expect("read from package.json");
    assert!(
        manifest.contains(&format!(r#""packageManager": "pnpm@{}""#, pnpm_config::PNPM_VERSION))
    );

    drop(root);
}

/// A new package inside an existing workspace follows the pin at the
/// workspace root, so `pnpm init` doesn't give it one of its own.
#[test]
fn a_new_workspace_member_is_not_pinned() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    fs::write(workspace.join("pnpm-workspace.yaml"), "packages:\n  - packages/*\n")
        .expect("write to pnpm-workspace.yaml");
    let member = workspace.join("packages/foo");
    fs::create_dir_all(&member).expect("create the workspace member directory");
    pacquet.with_current_dir(&member).with_arg("init").assert().success();

    assert_unpinned(&member);

    drop(root);
}

fn assert_unpinned(dir: &Path) {
    let manifest = fs::read_to_string(dir.join("package.json")).expect("read from package.json");
    assert!(!manifest.contains("devEngines"), "{manifest}");
    assert!(!manifest.contains("packageManager"), "{manifest}");
}
