use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pacquet_testing_utils::bin::CommandTempCwd;
use std::fs;

#[test]
fn should_list_registries() {
    let cwd = CommandTempCwd::init().add_mocked_registry();

    let cache_dir = cwd.npmrc_info.cache_dir.join("v11").join("metadata");
    fs::create_dir_all(cache_dir.join("registry.npmjs.org")).unwrap();
    fs::create_dir_all(cache_dir.join("registry.yarnpkg.com")).unwrap();

    let output = cwd
        .pacquet
        .with_arg("cache")
        .with_arg("list-registries")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let stdout = String::from_utf8(output).unwrap();
    assert!(stdout.contains("registry.npmjs.org"));
    assert!(stdout.contains("registry.yarnpkg.com"));
}

#[test]
fn should_list_packages() {
    let cwd = CommandTempCwd::init().add_mocked_registry();

    let cache_dir = cwd.npmrc_info.cache_dir.join("v11").join("metadata");
    fs::create_dir_all(cache_dir.join("registry.npmjs.org")).unwrap();
    fs::write(cache_dir.join("registry.npmjs.org").join("is-positive.jsonl"), "{}").unwrap();
    fs::write(cache_dir.join("registry.npmjs.org").join("is-negative.jsonl"), "{}").unwrap();

    let _output = cwd
        .pacquet
        .with_arg("cache")
        .with_arg("list")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
}

#[test]
fn should_delete_packages() {
    let cwd = CommandTempCwd::init().add_mocked_registry();

    let cache_dir = cwd.npmrc_info.cache_dir.join("v11").join("metadata");
    fs::create_dir_all(cache_dir.join("registry.npmjs.org")).unwrap();
    fs::write(cache_dir.join("registry.npmjs.org").join("is-positive.jsonl"), "{}").unwrap();
    fs::write(cache_dir.join("registry.npmjs.org").join("is-negative.jsonl"), "{}").unwrap();

    let _output = cwd
        .pacquet
        .with_arg("cache")
        .with_arg("delete")
        .with_arg("is-positive")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
}
