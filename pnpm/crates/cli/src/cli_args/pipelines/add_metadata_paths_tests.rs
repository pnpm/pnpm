use super::node_add_metadata_paths;
use pnpm_config::Config;
use std::path::PathBuf;

#[test]
fn uses_configured_node_metadata_locations() {
    let root = PathBuf::from("workspace");
    let node_manifest = root.join("package.json");
    let config = Config {
        lockfile_dir: Some(root.clone()),
        modules_dir: root.join("custom_modules"),
        virtual_store_dir: root.join("project-store"),
        global_virtual_store_dir: root.join("global-store"),
        enable_global_virtual_store: true,
        ..Config::default()
    };

    let metadata_paths = node_add_metadata_paths(&config, &node_manifest);
    eprintln!("metadata paths: {metadata_paths:#?}");

    assert_eq!(
        metadata_paths,
        vec![
            node_manifest,
            root.join("pnpm-lock.yaml"),
            root.join("project-store/lock.yaml"),
            root.join("custom_modules/.modules.yaml"),
        ],
    );
}
