use super::{Config, assert_eq, tempdir};

#[test]
fn patched_dependency_hashes_resolves_and_hashes_each_patch() {
    let workspace = tempdir().expect("workspace tempdir");
    let patch_path = workspace.path().join("patches").join("graceful-fs@4.2.11.patch");
    std::fs::create_dir_all(patch_path.parent().unwrap()).expect("create patches dir");
    std::fs::write(&patch_path, "--- a/index.js\n+++ b/index.js\n").expect("write patch");
    let expected = pnpm_patching::create_hex_hash_from_file(&patch_path).expect("hash patch file");

    let mut config = Config::new();
    assert!(config.patched_dependency_hashes().expect("no error").is_none(), "unset → None");

    config.workspace_dir = Some(workspace.path().to_path_buf());
    config.patched_dependencies = Some(indexmap::IndexMap::from([(
        "graceful-fs@4.2.11".to_string(),
        "patches/graceful-fs@4.2.11.patch".to_string(),
    )]));
    let hashes = config.patched_dependency_hashes().expect("hash").expect("present");
    assert_eq!(hashes.get("graceful-fs@4.2.11"), Some(&expected));
}
