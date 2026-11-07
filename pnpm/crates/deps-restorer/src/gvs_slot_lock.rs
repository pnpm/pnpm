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
        match fs::read_to_string(&self.state_file) {
            Ok(state) => {
                let mut phase = match state.as_str() {
                    "populating" => GlobalVirtualStoreSlotPhase::Populating,
                    "populated" => GlobalVirtualStoreSlotPhase::Populated,
                    "build-pending" => GlobalVirtualStoreSlotPhase::BuildPending,
                    "building" => GlobalVirtualStoreSlotPhase::Building,
                    "built" => GlobalVirtualStoreSlotPhase::Built,
                    _ => {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "invalid global virtual store slot state",
                        ));
                    }
                };
                if matches!(
                    phase,
                    GlobalVirtualStoreSlotPhase::Populated | GlobalVirtualStoreSlotPhase::Built
                ) {
                    phase = self.legacy_build_marker_phase(phase)?;
                }
                return Ok(GlobalVirtualStoreSlotState { phase, linked_archive });
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        match fs::metadata(completion_marker) {
            Ok(metadata) if metadata.is_file() => {}
            Ok(_) => {
                return Ok(GlobalVirtualStoreSlotState {
                    phase: GlobalVirtualStoreSlotPhase::Populating,
                    linked_archive,
                });
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(GlobalVirtualStoreSlotState {
                    phase: GlobalVirtualStoreSlotPhase::Populating,
                    linked_archive,
                });
            }
            Err(error) => return Err(error),
        }
        let phase = self.legacy_build_marker_phase(if requires_build {
            GlobalVirtualStoreSlotPhase::Built
        } else {
            GlobalVirtualStoreSlotPhase::Populated
        })?;
        Ok(GlobalVirtualStoreSlotState { phase, linked_archive })
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
                | GlobalVirtualStoreSlotPhase::Building
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
mod tests {
    use super::{GlobalVirtualStoreSlot, GlobalVirtualStoreSlotPhase};
    use tempfile::tempdir;

    #[test]
    fn slot_state_changes_only_through_the_locked_owner() {
        let root = tempdir().expect("temporary slot root");
        let key = "dep@1.0.0".parse().expect("package key");
        let slot = GlobalVirtualStoreSlot::acquire_path(root.path().join("slot"), &key)
            .expect("lock slot");
        let completion_marker = slot.package_path().join("package.json");

        assert_eq!(
            slot.state(&completion_marker, true).expect("read missing slot").phase,
            GlobalVirtualStoreSlotPhase::Missing,
        );
        std::fs::create_dir_all(slot.package_path()).expect("create package directory");
        std::fs::write(&completion_marker, "{}").expect("write completion marker");
        slot.finish_population(false).expect("mark slot populated");
        assert_eq!(
            slot.state(&completion_marker, false).expect("read populated slot").phase,
            GlobalVirtualStoreSlotPhase::Populated,
        );
        std::fs::write(slot.package_path().join(crate::NEEDS_BUILD_MARKER), "")
            .expect("write pending marker");
        slot.finish_population(true).expect("mark build pending");
        assert_eq!(
            slot.state(&completion_marker, true).expect("read pending build").phase,
            GlobalVirtualStoreSlotPhase::BuildPending,
        );
        slot.mark_build_started().expect("mark build started");
        assert_eq!(
            slot.state(&completion_marker, true).expect("read active build").phase,
            GlobalVirtualStoreSlotPhase::Building,
        );
        slot.mark_build_finished().expect("mark build finished");
        assert_eq!(
            slot.state(&completion_marker, true).expect("read finished build").phase,
            GlobalVirtualStoreSlotPhase::Built,
        );
        slot.record_linked_archive("sha512-archive").expect("record linked archive");
        assert_eq!(
            slot.state(&completion_marker, true)
                .expect("read linked archive")
                .linked_archive
                .as_deref(),
            Some("sha512-archive"),
        );
    }
}
