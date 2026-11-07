//! The owner of global virtual store slot state and its cross-process lock.

use crate::VirtualStoreLayout;
use derive_more::{Display, Error};
use miette::Diagnostic;
use pnpm_fs::DirLock;
use pnpm_lockfile::PackageKey;
use std::{fs, io, path::PathBuf, time::Duration};

/// The lock directory's name inside the slot directory, next to its
/// `node_modules`.
const SLOT_LOCK_DIR: &str = ".pnpm-build.lock";
const SLOT_STATE_FILE: &str = ".pnpm-slot-state";

fn parse_phase(state: &str) -> io::Result<GlobalVirtualStoreSlotPhase> {
    match state {
        "populating" => Ok(GlobalVirtualStoreSlotPhase::Populating),
        "populated" => Ok(GlobalVirtualStoreSlotPhase::Populated),
        "build-pending" => Ok(GlobalVirtualStoreSlotPhase::BuildPending),
        "building" => Ok(GlobalVirtualStoreSlotPhase::Building),
        "built" => Ok(GlobalVirtualStoreSlotPhase::Built),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid global virtual store slot state",
        )),
    }
}

/// How long each acquisition attempt waits before checking the lock again.
const WAIT: Duration = Duration::from_mins(10);

/// Comfortably above how long a build can legitimately take, so a holder
/// that cannot prove it is alive never has its lock stolen mid-build.
const ABANDONED_AFTER: Duration = Duration::from_mins(30);

#[derive(Debug, Display, Error, Diagnostic)]
#[display("Failed to lock global virtual store slot for {dep_path}: {source}")]
#[diagnostic(code(ERR_PNPM_GVS_SLOT_LOCK))]
pub struct GvsSlotLockError {
    dep_path: String,
    #[error(source)]
    source: io::Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlobalVirtualStoreSlotPhase {
    Missing,
    Populating,
    Populated,
    BuildPending,
    Building,
    Built,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlobalVirtualStoreSlotState {
    pub phase: GlobalVirtualStoreSlotPhase,
    pub linked_archive: Option<String>,
}

pub struct GlobalVirtualStoreSlot {
    path: PathBuf,
    package_path: PathBuf,
    needs_build_marker: PathBuf,
    state_file: PathBuf,
    _lock: DirLock,
}

/// Take the lock of `key`'s slot. `None` only when the global virtual
/// store is off. A timeout starts another wait; an acquisition error
/// prevents the caller from mutating the shared slot.
impl GlobalVirtualStoreSlot {
    pub fn acquire(
        layout: &VirtualStoreLayout,
        key: &PackageKey,
    ) -> Result<Option<Self>, GvsSlotLockError> {
        let Some(path) = layout.hashed_slot_dir(key) else {
            return Ok(None);
        };
        Self::acquire_path(path, key).map(Some)
    }

    pub fn acquire_existing(
        layout: &VirtualStoreLayout,
        key: &PackageKey,
    ) -> Result<Option<Self>, GvsSlotLockError> {
        let Some(path) = layout.hashed_slot_dir(key) else {
            return Ok(None);
        };
        Self::acquire_existing_path(path, key)
    }

    pub fn acquire_existing_path(
        path: PathBuf,
        key: &PackageKey,
    ) -> Result<Option<Self>, GvsSlotLockError> {
        if !path.is_dir() {
            return Ok(None);
        }
        Self::acquire_path(path, key).map(Some)
    }

    pub fn acquire_path(path: PathBuf, key: &PackageKey) -> Result<Self, GvsSlotLockError> {
        let lock = DirLock::acquire_waiting(&path.join(SLOT_LOCK_DIR), WAIT, ABANDONED_AFTER)
            .map_err(|source| GvsSlotLockError { dep_path: key.to_string(), source })?;
        let package_path = path.join("node_modules").join(key.name.to_string());
        let needs_build_marker = package_path.join(crate::NEEDS_BUILD_MARKER);
        let state_file = path.join(SLOT_STATE_FILE);
        Ok(Self { path, package_path, needs_build_marker, state_file, _lock: lock })
    }

    #[must_use]
    pub fn path(&self) -> &std::path::Path {
        &self.path
    }

    #[must_use]
    pub fn package_path(&self) -> &std::path::Path {
        &self.package_path
    }

    pub fn state(
        &self,
        completion_marker: &std::path::Path,
        requires_build: bool,
    ) -> io::Result<GlobalVirtualStoreSlotState> {
        let linked_archive = self.linked_archive();
        if !self.package_path.is_dir() {
            return Ok(GlobalVirtualStoreSlotState {
                phase: GlobalVirtualStoreSlotPhase::Missing,
                linked_archive,
            });
        }
        let phase = self
            .read_state_file()?
            .map_or_else(
                || self.legacy_state(completion_marker, requires_build),
                |phase| self.reconcile_legacy_marker(phase),
            )?;
        Ok(GlobalVirtualStoreSlotState { phase, linked_archive })
    }

    fn read_state_file(&self) -> io::Result<Option<GlobalVirtualStoreSlotPhase>> {
        match fs::read_to_string(&self.state_file) {
            Ok(state) => parse_phase(&state).map(Some),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        }
    }

    fn reconcile_legacy_marker(
        &self,
        phase: GlobalVirtualStoreSlotPhase,
    ) -> io::Result<GlobalVirtualStoreSlotPhase> {
        if matches!(
            phase,
            GlobalVirtualStoreSlotPhase::Populated | GlobalVirtualStoreSlotPhase::Built,
        ) {
            self.legacy_build_marker_phase(phase)
        } else {
            Ok(phase)
        }
    }

    fn legacy_state(
        &self,
        completion_marker: &std::path::Path,
        requires_build: bool,
    ) -> io::Result<GlobalVirtualStoreSlotPhase> {
        match fs::metadata(completion_marker) {
            Ok(metadata) if metadata.is_file() => {
                let absent_phase = if requires_build {
                    GlobalVirtualStoreSlotPhase::Built
                } else {
                    GlobalVirtualStoreSlotPhase::Populated
                };
                self.legacy_build_marker_phase(absent_phase)
            }
            Ok(_) => Ok(GlobalVirtualStoreSlotPhase::Populating),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                Ok(GlobalVirtualStoreSlotPhase::Populating)
            }
            Err(error) => Err(error),
        }
    }

    fn legacy_build_marker_phase(
        &self,
        absent_phase: GlobalVirtualStoreSlotPhase,
    ) -> io::Result<GlobalVirtualStoreSlotPhase> {
        match fs::read(&self.needs_build_marker) {
            Ok(content) if content == b"started" => Ok(GlobalVirtualStoreSlotPhase::Building),
            Ok(_) => Ok(GlobalVirtualStoreSlotPhase::BuildPending),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(absent_phase),
            Err(error) => Err(error),
        }
    }

    pub fn has_build_marker(&self) -> io::Result<bool> {
        let state = self.state(&self.package_path.join("package.json"), false)?;
        Ok(matches!(
            state.phase,
            GlobalVirtualStoreSlotPhase::Populating
                | GlobalVirtualStoreSlotPhase::BuildPending
                | GlobalVirtualStoreSlotPhase::Building,
        ))
    }

    pub fn begin_population(&self) -> io::Result<()> {
        self.write_state("populating")
    }

    pub fn finish_population(&self, needs_build: bool) -> io::Result<()> {
        self.write_state(if needs_build { "build-pending" } else { "populated" })
    }

    pub fn mark_build_started(&self) -> io::Result<()> {
        self.write_state("building")?;
        pnpm_fs::write_atomic(&self.needs_build_marker, b"started")
    }

    pub fn mark_build_pending(&self) -> io::Result<()> {
        self.write_state("build-pending")?;
        pnpm_fs::write_atomic(&self.needs_build_marker, b"")
    }

    pub fn mark_build_finished(&self) -> io::Result<()> {
        match fs::remove_file(&self.needs_build_marker) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
        }?;
        self.write_state("built")
    }

    #[must_use]
    pub fn linked_archive(&self) -> Option<String> {
        fs::read_to_string(self.path.join(
            crate::create_virtual_store::linked_archives::LINKED_ARCHIVE_RECORD,
        ))
        .ok()
        .map(|value| value.trim().to_string())
    }

    pub fn record_linked_archive(&self, integrity: &str) -> io::Result<()> {
        pnpm_fs::write_atomic(
            &self.path.join(crate::create_virtual_store::linked_archives::LINKED_ARCHIVE_RECORD),
            integrity.as_bytes(),
        )
    }

    fn write_state(&self, state: &str) -> io::Result<()> {
        pnpm_fs::write_atomic(&self.state_file, state.as_bytes())
    }
}

#[cfg(test)]
mod tests;
