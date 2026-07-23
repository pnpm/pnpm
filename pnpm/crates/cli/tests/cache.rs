use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pacquet_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
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

#[test]
fn should_delete_packages_from_all_metadata_dirs() {
    let cwd = CommandTempCwd::init().add_mocked_registry();

    let url_str = cwd.npmrc_info.mock_instance.url();
    let registry_name =
        pacquet_resolving_npm_resolver::mirror::get_registry_name(&url_str).unwrap();
    // A package can be cached under any metadata directory depending on the
    // resolution mode used at fetch time, so all of them must be cleared.
    let meta_dirs = [
        pacquet_resolving_npm_resolver::mirror::ABBREVIATED_META_DIR,
        pacquet_resolving_npm_resolver::mirror::FULL_META_DIR,
        pacquet_resolving_npm_resolver::mirror::FULL_FILTERED_META_DIR,
    ];
    for meta_dir in meta_dirs {
        let dir = cwd.npmrc_info.cache_dir.join(meta_dir).join(&registry_name);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("is-positive.jsonl"), "{}").unwrap();
    }

    cwd.pacquet.with_arg("cache").with_arg("delete").with_arg("is-positive").assert().success();

    for meta_dir in meta_dirs {
        let file =
            cwd.npmrc_info.cache_dir.join(meta_dir).join(&registry_name).join("is-positive.jsonl");
        assert!(!file.exists(), "expected {file:?} to be deleted");
    }
}

#[test]
fn should_view_package_cache() {
    let cwd = CommandTempCwd::init().add_mocked_registry();
    let cache_dir = cwd.npmrc_info.cache_dir.join("v11").join("metadata");
    let url_str = cwd.npmrc_info.mock_instance.url();
    let registry_name =
        pacquet_resolving_npm_resolver::mirror::get_registry_name(&url_str).unwrap();
    fs::create_dir_all(cache_dir.join(&registry_name)).unwrap();

    let package_jsonl = "{}\n{\
        \"name\":\"is-positive\",\
        \"dist-tags\":{\"latest\":\"1.0.0\"},\
        \"versions\":{\
            \"1.0.0\":{\
                \"name\":\"is-positive\",\
                \"version\":\"1.0.0\",\
                \"dist\":{\
                    \"integrity\":\"sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==\"\
                }\
            }\
        }\
    }";
    fs::write(cache_dir.join(&registry_name).join("is-positive.jsonl"), package_jsonl).unwrap();

    let output = cwd
        .pacquet
        .with_args(["cache", "view", "is-positive"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let stdout = String::from_utf8(output).unwrap();
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();

    let key = registry_name.replace('+', ":");
    assert!(json.get(&key).is_some());
    let info = json.get(&key).unwrap();
    assert!(info.get("cachedVersions").is_some());
    assert!(info.get("nonCachedVersions").is_some());
    assert!(info.get("cachedAt").is_some());
}

#[test]
fn import_populates_metadata_cache() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, cache_dir, .. } = npmrc_info;

    let manifest_path = workspace.join("package.json");
    fs::write(
        &manifest_path,
        serde_json::json!({
            "dependencies": {
                "@pnpm.e2e/pkg-with-1-dep": "100.0.0",
            },
        })
        .to_string(),
    )
    .expect("write package.json");

    pacquet.with_arg("import").assert().success();

    let registry_name =
        pacquet_resolving_npm_resolver::mirror::get_registry_name(&mock_instance.url()).unwrap();
    let cache_metadata_dir = cache_dir.join("v11").join("metadata").join(&registry_name);

    assert!(cache_metadata_dir.exists(), "metadata cache directory must exist");
    assert!(
        cache_metadata_dir.join("@pnpm.e2e/pkg-with-1-dep.jsonl").exists(),
        "cached metadata file for @pnpm.e2e/pkg-with-1-dep must exist",
    );
    assert!(
        cache_metadata_dir.join("@pnpm.e2e/dep-of-pkg-with-1-dep.jsonl").exists(),
        "cached metadata file for transitive dependency @pnpm.e2e/dep-of-pkg-with-1-dep must exist",
    );

    drop((root, mock_instance));
}
