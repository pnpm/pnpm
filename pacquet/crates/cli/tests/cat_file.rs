use assert_cmd::prelude::*;
use pacquet_testing_utils::bin::CommandTempCwd;
use std::fs;

#[test]
fn cat_file_works() {
    let CommandTempCwd { mut pacquet, root: _root, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();

    let store_dir = npmrc_info.store_dir;
    let files_dir = store_dir.join("v11").join("files");

    let content = "hello world";
    let hex = "309ecc489c12d6eb4cc40f50c902f2b4d0ed77ee511a7c7a9bcd3ca86d4cd86f989dd35bc5ff499670da34255b45b0cfd830e81f605dcf7dc5542e93ae9cd76f";
    let base64_hash =
        "MJ7MSJwS1utMxA9QyQLytNDtd+5RGnx6m808qG1M2G+YndNbxf9JlnDaNCVbRbDP2DDoH2Bdz33FVC6TrpzXbw==";

    let cafs_dir = files_dir.join(&hex[..2]);
    fs::create_dir_all(&cafs_dir).unwrap();
    let file_path = cafs_dir.join(&hex[2..]);
    fs::write(&file_path, content).unwrap();

    let output = pacquet.arg("cat-file").arg(format!("sha512-{base64_hash}")).assert().success();

    assert_eq!(String::from_utf8(output.get_output().stdout.clone()).unwrap(), "hello world");
}

#[test]
fn cat_file_works_with_binary() {
    let CommandTempCwd { mut pacquet, root: _root, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();

    let store_dir = npmrc_info.store_dir;
    let files_dir = store_dir.join("v11").join("files");

    let binary_content = b"\x89PNG\r\n\x1a\n\x00\x01\x02\x03\xff\xfe";
    let hex = "002fd6b03246626a0eb5d9a17b15b7d9d30fec368a04a8a809946750d66f22190ccf37a449207df3f98ac9bcf4554316e7beffa21dff3c80efc4b516f0c361be";
    let base64_hash =
        "AC/WsDJGYmoOtdmhexW32dMP7DaKBKioCZRnUNZvIhkMzzekSSB98/mKybz0VUMW577/oh3/PIDvxLUW8MNhvg==";

    let cafs_dir = files_dir.join(&hex[..2]);
    fs::create_dir_all(&cafs_dir).unwrap();
    let file_path = cafs_dir.join(&hex[2..]);
    fs::write(&file_path, binary_content).unwrap();

    let output = pacquet.arg("cat-file").arg(format!("sha512-{base64_hash}")).assert().success();

    assert_eq!(output.get_output().stdout, binary_content);
}

#[test]
fn should_fail_on_missing_hash() {
    let CommandTempCwd { mut pacquet, root: _root, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let output = pacquet.arg("cat-file").arg("sha512-MJ7MSJwS1utMxA9QyQLytNDtd+5RGnx6m808qG1M2G+YndNbxf9JlnDaNCVbRbDP2DDoH2Bdz33FVC6TrpzXbw==").assert().failure();
    let stderr = String::from_utf8_lossy(&output.get_output().stderr);
    assert!(stderr.contains("File not found in store"));
}

#[test]
fn should_fail_on_invalid_hash_format() {
    let CommandTempCwd { mut pacquet, root: _root, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let output = pacquet.arg("cat-file").arg("invalidhashformatwithoutdash").assert().failure();
    let stderr = String::from_utf8_lossy(&output.get_output().stderr);

    assert!(stderr.contains("Invalid hash format"));
}

#[test]
fn should_fail_on_invalid_base64() {
    let CommandTempCwd { mut pacquet, root: _root, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let output = pacquet.arg("cat-file").arg("sha512-InvalidBase64!!!").assert().failure();
    let stderr = String::from_utf8_lossy(&output.get_output().stderr);
    assert!(stderr.contains("Failed to decode base64 hash"));
}

#[test]
fn should_prevent_path_traversal() {
    let CommandTempCwd { mut pacquet, root: _root, .. } =
        CommandTempCwd::init().add_mocked_registry();
    // A crafted base64 payload that decodes to bytes resembling a path traversal like "../../etc/passwd".
    // "Li4vLi4vZXRjL3Bhc3N3ZA==" is base64 for "../../etc/passwd"
    let output = pacquet.arg("cat-file").arg("sha512-Li4vLi4vZXRjL3Bhc3N3ZA==").assert().failure();
    let stderr = String::from_utf8_lossy(&output.get_output().stderr);
    // It should fail to find the file (or decode as invalid depending on length), but it must NOT escape the store.
    assert!(stderr.contains("File not found in store"));
}
