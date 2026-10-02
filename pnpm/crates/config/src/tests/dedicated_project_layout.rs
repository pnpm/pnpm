use super::{Config, HostNoHome, PathBuf, assert_eq, fs, tempdir};

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

    config.anchor_dedicated_project(&project_dir, None);

    assert_eq!(config.install_state_dir, project_dir.join("node_modules").join(".pnpm"));
    assert_eq!(config.global_virtual_store_dir, global_virtual_store_dir);
}

#[test]
fn an_explicit_global_virtual_store_keeps_dedicated_project_state_local() {
    let root = tempdir().unwrap();
    fs::write(
        root.path().join("pnpm-workspace.yaml"),
        "enableGlobalVirtualStore: true\nvirtualStoreDir: links\nmodulesDir: vendor\n",
    )
    .unwrap();
    let mut config = Config::new().current::<HostNoHome>(root.path()).unwrap();
    let project_dir = root.path().join("packages/member");

    config.anchor_dedicated_project(&project_dir, None);

    assert_eq!(config.install_state_dir, project_dir.join("vendor/.pnpm"));
    assert_eq!(config.global_virtual_store_dir, root.path().join("links"));
}
