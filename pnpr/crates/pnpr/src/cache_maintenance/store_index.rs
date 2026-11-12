use super::{HashSet, Path};
use pnpm_fs::OpenDir;
use pnpm_store_dir::{PackageFilesIndex, StoreIndex, StoreIndexError, decode_package_files_index};
use std::{
    ffi::OsStr,
    io::{self, ErrorKind},
};

const STORE_V11_COMPONENTS: [&str; 2] = ["resolver-store", "v11"];
const STORE_INDEX_FAMILY: [&str; 4] =
    ["index.db", "index.db-wal", "index.db-shm", "index.db-journal"];

pub(super) fn store_index_family_bytes(cache_dir: &Path) -> u64 {
    let Ok(Some(store_root)) = open_store_v11_dir(cache_dir) else {
        return 0;
    };
    if STORE_INDEX_FAMILY
        .iter()
        .any(|name| {
            store_root
                .has_symlink_child(OsStr::new(name))
                .unwrap_or(true)
        })
    {
        return 0;
    }
    sqlite_bytes_in(&store_root)
}

pub(super) async fn prune_store_index_digests(
    cache_dir: &Path,
    removed_digests: HashSet<String>,
) -> io::Result<u64> {
    if removed_digests.is_empty() {
        return Ok(0);
    }
    let root = cache_dir.to_path_buf();
    tokio::task::spawn_blocking(move || prune_store_index_sync(&root, &removed_digests))
        .await
        .map_err(|err| io::Error::other(err.to_string()))?
}

fn prune_store_index_sync(cache_dir: &Path, removed_digests: &HashSet<String>) -> io::Result<u64> {
    let Some(store_root) = open_store_v11_dir(cache_dir)? else {
        return Ok(0);
    };
    let before = sqlite_bytes_in(&store_root);
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
    Ok(before.saturating_sub(sqlite_bytes_in(&store_root)))
}

fn open_store_v11_dir(cache_dir: &Path) -> io::Result<Option<OpenDir>> {
    let mut dir = match OpenDir::open(cache_dir) {
        Ok(dir) => dir,
        Err(err) if err.kind() == ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err),
    };
    for part in STORE_V11_COMPONENTS {
        if dir.has_symlink_child(OsStr::new(part))? {
            return Err(io::Error::new(
                ErrorKind::InvalidInput,
                format!("refusing symlinked cache directory component `{part}`"),
            ));
        }
        let Some(next) = dir.open_child_dir(OsStr::new(part))? else {
            return Ok(None);
        };
        dir = next;
    }
    Ok(Some(dir))
}

fn sqlite_bytes_in(dir: &OpenDir) -> u64 {
    STORE_INDEX_FAMILY
        .iter()
        .filter_map(|name| {
            dir.child_file_stat(OsStr::new(name))
                .ok()
                .flatten()
        })
        .map(|stat| stat.len)
        .sum()
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
