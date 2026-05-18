pub mod _utils;
pub use _utils::*;

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pacquet_testing_utils::{bin::CommandTempCwd, fs::get_filenames_in_folder};
use pretty_assertions::assert_eq;
use std::fs;

#[test]
fn should_create_package_json() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    pacquet.with_arg("init").assert().success();

    let manifest_path = workspace.join("package.json");
    dbg!(&manifest_path);

    eprintln!("Content of package.json");
    let package_json_content = fs::read_to_string(&manifest_path).expect("read from package.json");
    insta::assert_snapshot!(package_json_content);

    eprintln!("Created files");
    assert_eq!(get_filenames_in_folder(&workspace), ["package.json"]);

    drop(root); // cleanup
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

    drop(root); // cleanup
}
