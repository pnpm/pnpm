use super::{Config, PathBuf, assert_eq};

fn workspace() -> PathBuf {
    std::env::temp_dir().join("workspace")
}

fn global_virtual_store_config() -> Config {
    let mut config = Config::new();
    config.enable_global_virtual_store = true;
    config.anchor_lockfile_paths(&workspace());
    config
}

/// The project's own `.pnpm` holds its current lockfile and hidden hoisted
/// modules, while the slots stay in the store every project shares.
#[test]
fn a_dedicated_project_gets_its_own_internal_dir_under_a_global_virtual_store() {
    let mut config = global_virtual_store_config();
    let global_virtual_store_dir = config.global_virtual_store_dir.clone();
    let project_dir = workspace().join("packages/member");

    config.anchor_dedicated_project(&project_dir, None).expect("anchor the project");

    assert_eq!(config.virtual_store_dir, project_dir.join("node_modules").join(".pnpm"));
    assert_eq!(config.global_virtual_store_dir, global_virtual_store_dir);
}

/// Under a global virtual store, `virtualStoreDir` names the store's root,
/// which every project shares.
#[test]
fn an_explicit_virtual_store_dir_is_not_reanchored_under_a_global_virtual_store() {
    let mut config = global_virtual_store_config();
    let store_root = std::env::temp_dir().join("links");
    config.explicit_settings.insert(
        "virtualStoreDir".to_string(),
        store_root
            .to_string_lossy()
            .into_owned()
            .into(),
    );
    config.virtual_store_dir.clone_from(&store_root);

    config
        .anchor_dedicated_project(&workspace().join("packages/member"), None)
        .expect("anchor the project");

    assert_eq!(config.virtual_store_dir, store_root);
}
