use assert_cmd::prelude::*;
use pacquet_store_dir::store_index::StoreIndex;
use pacquet_testing_utils::bin::CommandTempCwd;

fn find_hash_fixture(store_index: &StoreIndex) -> (String, String, String) {
    let keys = store_index.keys().unwrap();
    assert!(!keys.is_empty(), "Store index should have at least one key");

    let entries = store_index.get_many(&keys).unwrap();
    for (_key, data) in entries {
        let Some(manifest) = &data.manifest else { continue };
        let Some(expected_name) = manifest.get("name").and_then(|value| value.as_str()) else {
            continue;
        };
        let Some(expected_version) = manifest.get("version").and_then(|value| value.as_str())
        else {
            continue;
        };
        if let Some(file) = data.files.values().next() {
            return (file.digest.clone(), expected_name.to_string(), expected_version.to_string());
        }
    }

    panic!("Should find a package hash with a non-empty name@version in the store index");
}

#[test]
fn find_hash_works() {
    let CommandTempCwd { mut pacquet, workspace, root: _root, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();

    // 1. Install a package to populate the store index
    pacquet.arg("add").arg("is-odd@3.0.1").assert().success();

    let store_dir = pacquet_store_dir::StoreDir::from(npmrc_info.store_dir);
    let store_index = StoreIndex::open_readonly_in(&store_dir).unwrap();
    let (valid_hash, expected_name, expected_version) = find_hash_fixture(&store_index);

    // 2. Run find-hash with the valid hash
    let mut pacquet2 = std::process::Command::cargo_bin("pacquet").unwrap();
    pacquet2.current_dir(&workspace);
    let output = pacquet2.arg("find-hash").arg(&valid_hash).assert().success();
    let stdout = String::from_utf8_lossy(&output.get_output().stdout);

    println!("STDOUT: {stdout}");

    // Output should contain the package name and version we extracted the hash from
    assert!(stdout.contains(&expected_name), "Expected stdout to contain name {expected_name}");
    assert!(
        stdout.contains(&expected_version),
        "Expected stdout to contain version {expected_version}",
    );
}

#[test]
fn should_fail_on_missing_hash() {
    let CommandTempCwd { mut pacquet, workspace, root: _root, .. } =
        CommandTempCwd::init().add_mocked_registry();
    // Install a package first so the store index exists.
    pacquet.arg("add").arg("is-odd@3.0.1").assert().success();
    // Use a valid-length hex string that no file matches. Create a fresh
    // command so the args from `add` don't carry over.
    let mut pacquet2 = std::process::Command::cargo_bin("pacquet").unwrap();
    pacquet2.current_dir(&workspace);
    let output = pacquet2.arg("find-hash").arg("ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff").assert().failure();
    let stderr = String::from_utf8_lossy(&output.get_output().stderr);
    assert!(stderr.contains("ERR_PNPM_INVALID_FILE_HASH"));
}

#[test]
fn should_fail_on_invalid_base64() {
    let CommandTempCwd { mut pacquet, root: _root, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let output = pacquet.arg("find-hash").arg("sha512-InvalidBase64!!!").assert().failure();
    let stderr = String::from_utf8_lossy(&output.get_output().stderr);
    assert!(stderr.contains("Failed to decode base64 hash"));
}

#[test]
fn should_fail_on_oversized_base64() {
    let CommandTempCwd { mut pacquet, root: _root, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let hash = format!("sha512-{}", "A".repeat(1_000));
    let output = pacquet.arg("find-hash").arg(hash).assert().failure();
    let stderr = String::from_utf8_lossy(&output.get_output().stderr);
    assert!(stderr.contains("sha512 base64 payload has 1000 character(s)"));
}

#[test]
fn find_hash_works_with_base64() {
    let CommandTempCwd { mut pacquet, workspace, root: _root, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();

    pacquet.arg("add").arg("is-odd@3.0.1").assert().success();

    let store_dir = pacquet_store_dir::StoreDir::from(npmrc_info.store_dir);
    let store_index = StoreIndex::open_readonly_in(&store_dir).unwrap();
    let (hex_hash, expected_name, expected_version) = find_hash_fixture(&store_index);

    // Convert hex to base64
    use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
    let bytes = (0..hex_hash.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex_hash[i..i + 2], 16).unwrap())
        .collect::<Vec<u8>>();
    let base64_hash = format!("sha512-{}", BASE64.encode(&bytes));

    let mut pacquet2 = std::process::Command::cargo_bin("pacquet").unwrap();
    pacquet2.current_dir(&workspace);
    let output = pacquet2.arg("find-hash").arg(&base64_hash).assert().success();
    let stdout = String::from_utf8_lossy(&output.get_output().stdout);

    println!("STDOUT: {stdout}");
    assert!(stdout.contains(&expected_name), "Expected stdout to contain name {expected_name}");
    assert!(
        stdout.contains(&expected_version),
        "Expected stdout to contain version {expected_version}",
    );
}
