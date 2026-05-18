use pacquet_store_dir::{CafsFileInfo, StoreDir, StoreIndex};
use std::{collections::BTreeMap, path::Path};

/// Snapshot-friendly view of every row in `<store>/v11/index.db`.
///
/// The outer key is the SQLite key (`"{integrity}\t{pkgId}"`). The inner
/// map is the package's files — one entry per path inside the tarball.
/// `checked_at` is scrubbed because its value depends on install time.
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
