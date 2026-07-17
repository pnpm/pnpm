use pacquet_store_dir::{CafsFileInfo, StoreDir, StoreIndex};
use std::{collections::BTreeMap, fmt::Write as _, fs, path::Path};

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

/// Minutes elapsed since 2022-03-01T00:00:00Z, for [`set_minimum_release_age`].
///
/// The mocked registry publishes `@pnpm.e2e/bravo-dep` at 1.0.0
/// (2022-02-01), 1.0.1 (2022-02-22), and 1.1.0 (2022-05-01, the `latest`
/// tag) — see `version_publish_time` in `pnpr/crates/pnpr-fixtures/src/lib.rs`
/// — so this cutoff makes 1.1.0 the only immature version.
#[must_use]
pub fn bravo_dep_mature_up_to_1_0_1_minimum_release_age() -> u64 {
    const CUTOFF_UNIX_SECS: u64 = 1_646_092_800; // 2022-03-01T00:00:00Z
    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock after epoch")
        .as_secs();
    (now_secs - CUTOFF_UNIX_SECS) / 60
}

/// Append a top-level `key: value` line to the `pnpm-workspace.yaml` the
/// harness already wrote. Appending is only valid while the harness never
/// writes the key itself (pnpm rejects duplicate top-level mapping keys),
/// so the guard assert fails loudly if that changes.
pub fn append_workspace_yaml_key(workspace: &Path, key: &str, value: impl std::fmt::Display) {
    let yaml_path = workspace.join("pnpm-workspace.yaml");
    let mut yaml = fs::read_to_string(&yaml_path).expect("read pnpm-workspace.yaml");
    let key_prefix = format!("{key}:");
    assert!(
        !yaml.lines().any(|line| line.starts_with(&key_prefix)),
        "pnpm-workspace.yaml already has a `{key}:` key — update this helper",
    );
    if !yaml.ends_with('\n') {
        yaml.push('\n');
    }
    writeln!(yaml, "{key}: {value}").unwrap();
    fs::write(&yaml_path, yaml).expect("write pnpm-workspace.yaml");
}

/// [`append_workspace_yaml_key`] for the `minimumReleaseAge` setting.
pub fn set_minimum_release_age(workspace: &Path, minutes: u64) {
    append_workspace_yaml_key(workspace, "minimumReleaseAge", minutes);
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
