use super::{BinInstall, StoreManifest, write_bins};
use std::collections::{BTreeMap, HashMap};

#[test]
fn stored_directories_bin_creates_commands_from_virtual_files() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path();
    let store = root.join("store");
    let digest = "a".repeat(128);
    let blob = pnpm_store_dir::loader_blob_path(&store.join("files"), &digest).unwrap();
    std::fs::create_dir_all(blob.parent().unwrap()).unwrap();
    std::fs::write(&blob, r#"{"name":"tool","directories":{"bin":"./commands"}}"#).unwrap();
    let config = pnpm_config::Config {
        modules_dir: root.join(".pnpm"),
        install_state_dir: root.join(".pnpm"),
        ..pnpm_config::Config::default()
    };
    let manifest = StoreManifest {
        version: 1,
        store_dir: store,
        packages: BTreeMap::from([
            (
                ".".into(),
                super::super::StorePackage {
                    root: Some(root.to_path_buf()),
                    dependencies: BTreeMap::from([("tool".into(), "tool@1".into())]),
                    files: None,
                    resolution: None,
                },
            ),
            (
                "tool@1".into(),
                super::super::StorePackage {
                    files: Some(BTreeMap::from([
                        ("package.json".into(), digest.clone()),
                        ("commands/hello".into(), digest.clone()),
                        ("commands/nested/another".into(), digest),
                    ])),
                    root: None,
                    resolution: None,
                    dependencies: BTreeMap::new(),
                },
            ),
        ]),
    };
    let importers = HashMap::from([(".".into(), pnpm_lockfile::ProjectSnapshot::default())]);
    write_bins(&BinInstall { config: &config, root, importers: &importers }, &manifest).unwrap();
    assert!(root.join(".pnpm/.bin/hello").is_file());
    assert!(root.join(".pnpm/.bin/another").is_file());
    assert!(!root.join(".pnpm-loader").exists());
}
