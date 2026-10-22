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
        modules_dir: root.join("node_modules"),
        install_state_dir: root.join("node_modules/.pnpm"),
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
    write_bins(
        &BinInstall {
            trusted_importer_ids: &std::collections::HashSet::new(),
            config: &config,
            root,
            importers: &importers,
        },
        &manifest,
    )
    .unwrap();
    assert!(root.join("node_modules/.bin/hello").is_file());
    assert!(root.join("node_modules/.bin/another").is_file());
    assert!(!root.join("node_modules/.pnpm/.pnpm-loader").exists());
}

#[test]
fn loaded_bins_reject_untrusted_importer_paths_before_writing() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path();
    let config = pnpm_config::Config::default();
    let manifest =
        StoreManifest { version: 1, store_dir: root.join("store"), packages: BTreeMap::new() };
    let trusted = std::collections::HashSet::new();
    for importer in ["../outside", "/outside", "C:/outside", "packages/./child", "packages//child"]
    {
        let importers =
            HashMap::from([(importer.to_string(), pnpm_lockfile::ProjectSnapshot::default())]);
        let inputs = BinInstall {
            config: &config,
            root,
            importers: &importers,
            trusted_importer_ids: &trusted,
        };
        assert!(write_bins(&inputs, &manifest).is_err(), "{importer}");
    }
    let importers =
        HashMap::from([("../configured".into(), pnpm_lockfile::ProjectSnapshot::default())]);
    let trusted = std::collections::HashSet::from(["../configured".into()]);
    write_bins(
        &BinInstall {
            config: &config,
            root,
            importers: &importers,
            trusted_importer_ids: &trusted,
        },
        &manifest,
    )
    .unwrap();
    assert_eq!(std::fs::read_dir(root).unwrap().count(), 0);
}
