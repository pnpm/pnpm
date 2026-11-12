use super::HashSet;
use pnpm_fs::OpenDir;
use pnpm_store_dir::{
    PackageFilesIndex, STORE_VERSION, StoreIndex, StoreIndexError, decode_package_files_index,
};
use std::{
    ffi::OsStr,
    io::{self, ErrorKind},
    sync::Arc,
};

const STORE_DIR_COMPONENTS: [&str; 2] = ["resolver-store", STORE_VERSION];
const STORE_INDEX_FAMILY: [&str; 4] =
    ["index.db", "index.db-wal", "index.db-shm", "index.db-journal"];

pub(super) fn store_index_family_bytes(cache_root: &OpenDir) -> io::Result<u64> {
    let Some(first) = cache_root.open_child_dir(OsStr::new(STORE_DIR_COMPONENTS[0]))? else {
        return Ok(0);
    };
    let Some(store_dir) = first.open_child_dir(OsStr::new(STORE_DIR_COMPONENTS[1]))? else {
        return Ok(0);
    };
    sqlite_bytes_in(&store_dir)
}

pub(super) async fn prune_store_index_digests(
    cache_root: &Arc<OpenDir>,
    removed_digests: HashSet<String>,
) -> io::Result<u64> {
    if removed_digests.is_empty() {
        return Ok(0);
    }
    let root = Arc::clone(cache_root);
    tokio::task::spawn_blocking(move || prune_store_index_sync(&root, &removed_digests))
        .await
        .map_err(|err| io::Error::other(err.to_string()))?
}

fn prune_store_index_sync(
    cache_root: &OpenDir,
    removed_digests: &HashSet<String>,
) -> io::Result<u64> {
    let Some(store_root) = open_store_dir(cache_root)? else {
        return Ok(0);
    };
    let before = sqlite_bytes_in(&store_root)?;
    let Some(mut index) = StoreIndex::open_existing_at(&store_root).map_err(io::Error::other)?
    else {
        return Ok(0);
    };
    let mut keys = Vec::new();
    index
        .for_each_raw(|key, data| {
            if let Ok(pkg) = decode_package_files_index(&data)
                && package_references_removed_digest(&pkg, removed_digests)
            {
                keys.push(key);
            }
            Ok::<(), StoreIndexError>(())
        })
        .map_err(io::Error::other)?;
    index.delete_many(&keys).map_err(io::Error::other)?;
    drop(index);
    Ok(before.saturating_sub(sqlite_bytes_in(&store_root)?))
}

fn open_store_dir(cache_root: &OpenDir) -> io::Result<Option<OpenDir>> {
    let first_name = STORE_DIR_COMPONENTS[0];
    if cache_root.has_symlink_child(OsStr::new(first_name))? {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            format!("refusing symlinked cache directory component `{first_name}`"),
        ));
    }
    let Some(first) = cache_root.open_child_dir(OsStr::new(first_name))? else {
        return Ok(None);
    };
    let second_name = STORE_DIR_COMPONENTS[1];
    if first.has_symlink_child(OsStr::new(second_name))? {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            format!("refusing symlinked cache directory component `{second_name}`"),
        ));
    }
    first.open_child_dir(OsStr::new(second_name))
}

fn sqlite_bytes_in(dir: &OpenDir) -> io::Result<u64> {
    let mut total = 0u64;
    for name in STORE_INDEX_FAMILY {
        if let Some(stat) = dir.child_file_stat(OsStr::new(name))? {
            total = total.saturating_add(stat.len);
        }
    }
    Ok(total)
}

fn package_references_removed_digest(
    pkg: &PackageFilesIndex,
    removed_digests: &HashSet<String>,
) -> bool {
    pkg.files
        .values()
        .any(|file| removed_digests.contains(&file.digest))
        || pkg.side_effects
            .iter()
            .flatten()
            .flat_map(|(_, diff)| diff.added.iter().flatten())
            .any(|(_, file)| removed_digests.contains(&file.digest))
}
