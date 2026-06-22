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
    let url_str = cwd.npmrc_info.mock_instance.url();
    let registry_name =
        pacquet_resolving_npm_resolver::mirror::get_registry_name(&url_str).unwrap();
    fs::create_dir_all(cache_dir.join(&registry_name)).unwrap();
    fs::write(cache_dir.join(&registry_name).join("is-positive.jsonl"), "{}").unwrap();
    fs::write(cache_dir.join(&registry_name).join("is-negative.jsonl"), "{}").unwrap();

    let output = cwd
        .pacquet
        .with_arg("cache")
        .with_arg("list")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let stdout = String::from_utf8_lossy(&output);
    assert!(stdout.contains(&format!("{registry_name}/is-positive.jsonl")));
    assert!(stdout.contains(&format!("{registry_name}/is-negative.jsonl")));
}

#[test]
fn should_list_only_files_not_directories() {
    let cwd = CommandTempCwd::init().add_mocked_registry();

    let cache_dir = cwd.npmrc_info.cache_dir.join("v11").join("metadata");
    let url_str = cwd.npmrc_info.mock_instance.url();
    let registry_name =
        pacquet_resolving_npm_resolver::mirror::get_registry_name(&url_str).unwrap();
    fs::create_dir_all(cache_dir.join(&registry_name)).unwrap();
    fs::write(cache_dir.join(&registry_name).join("is-positive.jsonl"), "{}").unwrap();
    // A scoped package lives in its own directory, which the glob also matches.
    // Only the file underneath it, not the directory itself, should be listed.
    fs::create_dir_all(cache_dir.join(&registry_name).join("@scope")).unwrap();
    fs::write(cache_dir.join(&registry_name).join("@scope").join("foo.jsonl"), "{}").unwrap();

    let output = cwd
        .pacquet
        .with_arg("cache")
        .with_arg("list")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let stdout = String::from_utf8_lossy(&output);
    assert!(stdout.contains(&format!("{registry_name}/is-positive.jsonl")));
    assert!(stdout.contains(&format!("{registry_name}/@scope/foo.jsonl")));
    let scope_dir = format!("{registry_name}/@scope");
    assert!(
        !stdout.lines().any(|line| line == scope_dir),
        "directory entry {scope_dir:?} should not be listed, got: {stdout}",
    );
}

#[test]
fn should_delete_packages() {
    let cwd = CommandTempCwd::init().add_mocked_registry();

    let cache_dir = cwd.npmrc_info.cache_dir.join("v11").join("metadata");
    let url_str = cwd.npmrc_info.mock_instance.url();
    let registry_name =
        pacquet_resolving_npm_resolver::mirror::get_registry_name(&url_str).unwrap();
    fs::create_dir_all(cache_dir.join(&registry_name)).unwrap();
    fs::write(cache_dir.join(&registry_name).join("is-positive.jsonl"), "{}").unwrap();
    fs::write(cache_dir.join(&registry_name).join("is-negative.jsonl"), "{}").unwrap();

    let output = cwd
        .pacquet
        .with_arg("cache")
        .with_arg("delete")
        .with_arg("is-positive")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let stdout = String::from_utf8_lossy(&output);
    assert!(stdout.contains(&format!("{registry_name}/is-positive.jsonl")));
    assert!(!cache_dir.join(&registry_name).join("is-positive.jsonl").exists());
    assert!(cache_dir.join(&registry_name).join("is-negative.jsonl").exists());
}
