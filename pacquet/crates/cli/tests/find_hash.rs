use assert_cmd::prelude::*;
use pacquet_store_dir::store_index::StoreIndex;
use pacquet_testing_utils::bin::CommandTempCwd;

#[test]
fn find_hash_works() {
    let CommandTempCwd { mut pacquet, workspace, root: _root, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();

    // 1. Install a package to populate the store index
    pacquet.arg("add").arg("is-odd@3.0.1").assert().success();

    let store_dir = pacquet_store_dir::StoreDir::from(npmrc_info.store_dir);
    let store_index = StoreIndex::open_readonly_in(&store_dir).unwrap();
    let keys = store_index.keys().unwrap();
    assert!(!keys.is_empty(), "Store index should have at least one key");

    // Find a valid hash
    let mut valid_hash = String::new();
    let entries = store_index.get_many(&keys).unwrap();
    for (_key, data) in entries {
        if let Some(file) = data.files.values().next() {
            valid_hash = file.digest.clone();
        }
        if !valid_hash.is_empty() {
            break;
        }
    }
    assert!(!valid_hash.is_empty(), "Should find a valid hash in the store index");

    // 2. Run find-hash with the valid hash
    let mut pacquet2 = std::process::Command::cargo_bin("pacquet").unwrap();
    pacquet2.current_dir(&workspace);
    let output = pacquet2.arg("find-hash").arg(&valid_hash).assert().success();
    let stdout = String::from_utf8_lossy(&output.get_output().stdout);

    println!("STDOUT: {stdout}");

    // Output should contain is-odd
    assert!(stdout.contains("is-odd"));
    assert!(stdout.contains("3.0.1"));
}

#[test]
fn should_fail_on_missing_hash() {
    let CommandTempCwd { mut pacquet, root: _root, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let output = pacquet.arg("find-hash").arg("sha512-MJ7MSJwS1utMxA9QyQLytNDtd+5RGnx6m808qG1M2G+YndNbxf9JlnDaNCVbRbDP2DDoH2Bdz33FVC6TrpzXbw==").assert().failure();
    let stderr = String::from_utf8_lossy(&output.get_output().stderr);
    assert!(stderr.contains("INVALID_FILE_HASH"));
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
fn find_hash_works_with_base64() {
    let CommandTempCwd { mut pacquet, workspace, root: _root, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();

    pacquet.arg("add").arg("is-odd@3.0.1").assert().success();

    let store_dir = pacquet_store_dir::StoreDir::from(npmrc_info.store_dir);
    let store_index = StoreIndex::open_readonly_in(&store_dir).unwrap();
    let keys = store_index.keys().unwrap();

    let mut hex_hash = String::new();
    let entries = store_index.get_many(&keys).unwrap();
    for (_key, data) in entries {
        if let Some(file) = data.files.values().next() {
            hex_hash = file.digest.clone();
            break;
        }
    }

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
    assert!(stdout.contains("is-odd"));
    assert!(stdout.contains("3.0.1"));
}
