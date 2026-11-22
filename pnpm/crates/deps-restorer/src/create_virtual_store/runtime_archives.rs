use super::{
    CreateVirtualStoreError,
    SnapshotCacheKey,
};
use crate::VirtualStoreLayout;
use pnpm_fs::OpenDir;
use pnpm_lockfile::PackageKey;
use std::{
    collections::{
        HashMap,
        HashSet,
    },
    io,
    path::Path,
};

const RECORD: &str = ".pnpm-runtime-archive";

/// Local runtime slots to replace, keyed by snapshot with the selected archive's store index key.
#[derive(Default)]
pub(super) struct RuntimeArchives(HashMap<PackageKey, String>);

impl RuntimeArchives {
    pub(super) fn schedule_if_stale(
        &mut self,
        layout: &VirtualStoreLayout,
        key: &PackageKey,
        selected: Option<&Result<SnapshotCacheKey, CreateVirtualStoreError>>,
    ) -> Result<bool, CreateVirtualStoreError> {
        let Some(archive) = selected
            .map(Result::as_ref)
            .and_then(Result::ok)
            .and_then(|key| key.value.as_ref())
        else {
            // Let the fetcher report the unmatched target.
            return Ok(true);
        };
        let path = layout.slot_dir(key);
        if record_matches(&path, archive.as_bytes())
            .map_err(|error| CreateVirtualStoreError::InspectVirtualStoreSlot { path, error })?
        {
            return Ok(false);
        }
        self.0.insert(key.clone(), archive.clone());
        Ok(true)
    }

    pub(super) fn must_replace(&self, key: &PackageKey) -> bool {
        self.0.contains_key(key)
    }

    pub(super) fn record_installed(
        &self,
        layout: &VirtualStoreLayout,
        failed: &HashSet<PackageKey>,
    ) -> Result<(), CreateVirtualStoreError> {
        for (key, archive) in &self.0 {
            if failed.contains(key) {
                continue;
            }
            let path = layout.slot_dir(key);
            open_slot(&path)
                .and_then(|slot| slot.write_child_file(RECORD.as_ref(), archive.as_bytes()))
                .map_err(|error| CreateVirtualStoreError::RecordRuntimeArchive { path, error })?;
        }
        Ok(())
    }
}

fn open_slot(path: &Path) -> io::Result<OpenDir> {
    let parent = path.parent().ok_or(io::ErrorKind::InvalidInput)?;
    let name = path.file_name().ok_or(io::ErrorKind::InvalidInput)?;
    OpenDir::open(parent)?.open_child_dir(name)?.ok_or_else(|| io::ErrorKind::NotFound.into())
}

fn record_matches(path: &Path, archive: &[u8]) -> io::Result<bool> {
    let slot = match open_slot(path) {
        Ok(slot) => slot,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error),
    };
    let name = RECORD.as_ref();
    match slot.read_child_file_bounded(name, archive.len()) {
        Ok(Some(record)) if record == archive => return Ok(true),
        Ok(None) => return Ok(false),
        Ok(Some(_)) => {}
        Err(error) if error.kind() == io::ErrorKind::InvalidData => {}
        Err(error) => return Err(error),
    }
    // An interrupted replacement must leave the slot unrecorded.
    slot.remove_child_file(name)?;
    Ok(false)
}
