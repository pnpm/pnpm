use super::{StoreIndexError, TEMP_FILE_PREFIX, decode_entry};
use pnpm_fs::OpenDir;
use std::{
    ffi::OsStr,
    io::{self, ErrorKind},
    sync::Arc,
};

#[derive(Clone)]
pub(in crate::store_index) struct AnchoredFilesRoot {
    pub(super) store_root: Arc<OpenDir>,
    pub(super) subdir: &'static str,
}

pub(super) fn read_entry(
    anchored: &AnchoredFilesRoot,
    hex: &str,
) -> Result<Option<Vec<u8>>, StoreIndexError> {
    let Some(index_dir) = open_index_subdir_for_read(anchored)? else {
        return Ok(None);
    };
    let shard = &hex[..2];
    let entry = &hex[2..];
    ensure_no_read_symlink(&index_dir, shard)?;
    let Some(shard_dir) = index_dir
        .open_child_dir(OsStr::new(shard))
        .map_err(|source| read_err(&index_dir, source))?
    else {
        return Ok(None);
    };
    ensure_no_read_symlink(&shard_dir, entry)?;
    shard_dir
        .read_child_file(OsStr::new(entry))
        .map_err(|source| StoreIndexError::ReadEntry { path: shard_dir.path().join(entry), source })
}

pub(super) fn remove_entry(anchored: &AnchoredFilesRoot, hex: &str) -> Result<(), StoreIndexError> {
    let Some(index_dir) = open_index_subdir_for_write(anchored)? else {
        return Ok(());
    };
    let shard = &hex[..2];
    let entry = &hex[2..];
    ensure_no_write_symlink(&index_dir, shard)?;
    let Some(shard_dir) = index_dir
        .open_child_dir(OsStr::new(shard))
        .map_err(|source| write_err(&index_dir, shard, source))?
    else {
        return Ok(());
    };
    ensure_no_write_symlink(&shard_dir, entry)?;
    shard_dir
        .remove_child_file(OsStr::new(entry))
        .map(|_| ())
        .map_err(|source| write_err(&shard_dir, entry, source))
}

pub(super) fn for_each_decoded<VisitError>(
    anchored: &AnchoredFilesRoot,
    mut visit: impl FnMut(String, Vec<u8>) -> Result<(), VisitError>,
) -> Result<(), VisitError>
where
    VisitError: From<StoreIndexError>,
{
    let Some(index_dir) = open_index_subdir_for_read(anchored)? else {
        return Ok(());
    };
    let shard_names = index_dir.entry_names().map_err(|source| read_err(&index_dir, source))?;
    for shard_name in shard_names {
        ensure_no_read_symlink_os(&index_dir, &shard_name)?;
        let Some(shard_dir) =
            index_dir.open_child_dir(&shard_name).map_err(|source| read_err(&index_dir, source))?
        else {
            continue;
        };
        visit_shard_entries(&shard_dir, &mut visit)?;
    }
    Ok(())
}

pub(super) fn collect_matching(
    anchored: &AnchoredFilesRoot,
    matches: impl Fn(&str) -> bool + Sync,
) -> Result<Vec<(String, Vec<u8>)>, StoreIndexError> {
    let mut found = Vec::new();
    for_each_decoded::<StoreIndexError>(anchored, |key, value| {
        if matches(&key) {
            found.push((key, value));
        }
        Ok(())
    })?;
    found.sort_unstable_by(|(left, _), (right, _)| left.cmp(right));
    Ok(found)
}

fn visit_shard_entries<VisitError>(
    shard_dir: &OpenDir,
    visit: &mut impl FnMut(String, Vec<u8>) -> Result<(), VisitError>,
) -> Result<(), VisitError>
where
    VisitError: From<StoreIndexError>,
{
    let entry_names = shard_dir.entry_names().map_err(|source| read_err(shard_dir, source))?;
    for entry_name in entry_names {
        if is_temp_name(&entry_name) {
            continue;
        }
        ensure_no_read_symlink_os(shard_dir, &entry_name)?;
        let bytes = shard_dir
            .read_child_file(&entry_name)
            .map_err(|source| StoreIndexError::ReadEntry {
                path: shard_dir.path().join(&entry_name),
                source,
            })?;
        if let Some(raw) = bytes
            && let Some((key, value)) = decode_entry(&raw)
        {
            visit(key.to_string(), value.to_vec())?;
        }
    }
    Ok(())
}

fn is_temp_name(name: &OsStr) -> bool {
    name.to_str()
        .is_none_or(|raw| raw.starts_with(TEMP_FILE_PREFIX))
}

fn open_index_subdir_for_read(
    anchored: &AnchoredFilesRoot,
) -> Result<Option<OpenDir>, StoreIndexError> {
    ensure_no_read_symlink(&anchored.store_root, anchored.subdir)?;
    anchored.store_root
        .open_child_dir(OsStr::new(anchored.subdir))
        .map_err(|source| read_err(&anchored.store_root, source))
}

fn open_index_subdir_for_write(
    anchored: &AnchoredFilesRoot,
) -> Result<Option<OpenDir>, StoreIndexError> {
    ensure_no_write_symlink(&anchored.store_root, anchored.subdir)?;
    anchored.store_root
        .open_child_dir(OsStr::new(anchored.subdir))
        .map_err(|source| write_err(&anchored.store_root, anchored.subdir, source))
}

fn ensure_no_read_symlink(parent: &OpenDir, name: &str) -> Result<(), StoreIndexError> {
    ensure_no_read_symlink_os(parent, OsStr::new(name))
}

fn ensure_no_read_symlink_os(parent: &OpenDir, name: &OsStr) -> Result<(), StoreIndexError> {
    if parent.has_symlink_child(name).map_err(|source| read_err(parent, source))? {
        return Err(StoreIndexError::ReadEntry {
            path: parent.path().join(name),
            source: symlink_io_error(),
        });
    }
    Ok(())
}

fn ensure_no_write_symlink(parent: &OpenDir, name: &str) -> Result<(), StoreIndexError> {
    if parent
        .has_symlink_child(OsStr::new(name))
        .map_err(|source| write_err(parent, name, source))?
    {
        return Err(write_err(parent, name, symlink_io_error()));
    }
    Ok(())
}

fn symlink_io_error() -> io::Error {
    io::Error::new(ErrorKind::InvalidInput, "refusing symlinked store index entry")
}

fn read_err(dir: &OpenDir, source: io::Error) -> StoreIndexError {
    StoreIndexError::ReadEntry { path: dir.path().to_path_buf(), source }
}

fn write_err(dir: &OpenDir, child: &str, source: io::Error) -> StoreIndexError {
    StoreIndexError::WriteEntry { path: dir.path().join(child), source }
}
