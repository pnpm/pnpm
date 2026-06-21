use assert_cmd::prelude::*;
use pacquet_testing_utils::bin::CommandTempCwd;
use std::fs;

#[test]
fn cat_file_works() {
    let CommandTempCwd { mut pacquet, root: _root, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();

    // Create a mock store directory
    let store_dir = npmrc_info.store_dir;
    let files_dir = store_dir.join("v11").join("files");

    // Using some valid hash bytes.
    // "hello world" -> sha512: 309ecc489c12d6eb4cc40f50c902f2b4d0ed77ee511a7c7a9bcd3ca86d4cd86f989dd35bc5ff499670da34255b45b0cfd830e81f605dcf7dc5542e93ae9cd76f
    // which in base64 is: MJ7MSJwS1utMxA9QyQLytNDtd+5RGnx6m808qG1M2G+YndNbxf9JlnDaNCVbRbDP2DDoH2Bdz33FVC6TrpzXbw==
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
