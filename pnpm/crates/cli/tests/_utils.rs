use pacquet_store_dir::{CafsFileInfo, StoreDir, StoreIndex};
use std::{collections::BTreeMap, fs, path::Path};

/// Flip the `enableGlobalVirtualStore` key in the `pnpm-workspace.yaml`
/// that [`pacquet_testing_utils::bin::CommandTempCwd::add_mocked_registry`]
/// populated with `storeDir` / `cacheDir` / `enableGlobalVirtualStore: false`.
/// The replacement is in-place rather than appended so the file stays
/// valid YAML (pnpm rejects duplicate top-level mapping keys).
pub fn enable_gvs_in_workspace_yaml(workspace: &Path, extra_yaml: &str) {
    let yaml_path = workspace.join("pnpm-workspace.yaml");
    let yaml = fs::read_to_string(&yaml_path).expect("read pnpm-workspace.yaml");
    let flipped = yaml.replace("enableGlobalVirtualStore: false", "enableGlobalVirtualStore: true");
    assert_ne!(
        flipped, yaml,
        "expected the default `enableGlobalVirtualStore: false` line written by \
         `CommandTempCwd::add_mocked_registry` — has the helper changed?",
    );
    let mut yaml = flipped;
    if !yaml.ends_with('\n') {
        yaml.push('\n');
    }
    yaml.push_str(extra_yaml);
    fs::write(&yaml_path, yaml).expect("write pnpm-workspace.yaml");
}

/// Snapshot-friendly view of every row in `<store>/v11/index.db`.
///
/// The outer key is the `SQLite` key (`"{integrity}\t{pkgId}"`). The inner
/// map is the package's files — one entry per path inside the tarball.
/// `checked_at` is scrubbed because its value depends on install time.
#[must_use]
pub fn index_file_contents(store_dir: &Path) -> BTreeMap<String, BTreeMap<String, CafsFileInfo>> {
    let store = StoreDir::new(store_dir);
    // open_readonly: we're just reading for snapshot assertions, so don't
    // create WAL sidecars or otherwise mutate the store.
    let index = StoreIndex::open_readonly_in(&store).expect("open v11 index.db");

    let mut out = BTreeMap::new();
    for key in index.keys().expect("list index keys") {
        let row = index.get(&key).expect("read index row").expect("row disappeared");
        let files = row
            .files
            .into_iter()
            .map(|(filename, mut info)| {
                info.checked_at = None;
                (filename, info)
            })
            .collect();
        out.insert(key, files);
    }
    out
}
