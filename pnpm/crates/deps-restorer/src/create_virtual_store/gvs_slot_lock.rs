//! The owner of global virtual store slot state and its cross-process lock.

pub(crate) use acquire::{slot_retry_deadline, slot_retry_expired};
pub use path_codec::GVS_LOCKED_SLOT_PATHS_ENV;
pub(crate) use path_codec::{encode_slot_lock_paths, inherited_slot_locks};
mod acquire;
mod ordered;
mod path_codec;
use crate::VirtualStoreLayout;
use derive_more::{Display, Error};
use miette::Diagnostic;
use path_codec::slot_lock_is_inherited;
use pnpm_fs::DirLock;
use pnpm_lockfile::PackageKey;
use std::{fs, io, path::PathBuf, time::Duration};

/// The lock directory's name inside the slot directory, next to its
/// `node_modules`.
const SLOT_LOCK_DIR: &str = ".pnpm-build.lock";

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

impl GlobalVirtualStoreSlotPhase {
    pub fn parse(state: &str) -> io::Result<Self> {
        match state {
            "populating" => Ok(Self::Populating),
            "populated" => Ok(Self::Populated),
            "build-pending" => Ok(Self::BuildPending),
            "building" => Ok(Self::Building),
            "built" => Ok(Self::Built),
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid global virtual store slot state",
            )),
        }
    }

    #[must_use]
    pub fn as_str(self) -> Option<&'static str> {
        match self {
            Self::Missing => None,
            Self::Populating => Some("populating"),
            Self::Populated => Some("populated"),
            Self::BuildPending => Some("build-pending"),
            Self::Building => Some("building"),
            Self::Built => Some("built"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildRequirement {
    Required,
    NotRequired,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlobalVirtualStoreSlotState {
    pub phase: GlobalVirtualStoreSlotPhase,
    pub linked_archive: Option<String>,
}

/// The outcome of [`GlobalVirtualStoreSlot::try_acquire`].
pub enum SlotAttempt {
    /// The lock is held, or the slot is not in the global virtual store.
    Acquired(Option<Box<GlobalVirtualStoreSlot>>),
    Busy,
}

pub struct GlobalVirtualStoreSlot {
    path: PathBuf,
    package_path: PathBuf,
    needs_build_marker: PathBuf,
    state_file: PathBuf,
    lock: Option<DirLock>,
}

impl GlobalVirtualStoreSlot {
    /// Take the lock of `key`'s slot. `None` only when the global virtual
    /// store is off. A timeout starts another wait; an acquisition error
    /// prevents the caller from mutating the shared slot.
    pub fn acquire(
        layout: &VirtualStoreLayout,
        key: &PackageKey,
    ) -> Result<Option<Self>, GvsSlotLockError> {
        let Some(path) = layout.hashed_slot_dir(key) else {
            return Ok(None);
        };
        if layout.frozen_store() {
            return Err(GvsSlotLockError {
                dep_path: key.to_string(),
                source: io::Error::new(io::ErrorKind::PermissionDenied, "store is read-only"),
            });
        }
        Self::acquire_path(path, key).map(Some)
    }

    /// [`Self::acquire`] without waiting: [`SlotAttempt::Busy`] when another
    /// install holds the lock.
    pub fn try_acquire(
        layout: &VirtualStoreLayout,
        key: &PackageKey,
    ) -> Result<SlotAttempt, GvsSlotLockError> {
        let Some(path) = layout.hashed_slot_dir(key) else {
            return Ok(SlotAttempt::Acquired(None));
        };
        if layout.frozen_store() {
            return Err(GvsSlotLockError {
                dep_path: key.to_string(),
                source: io::Error::new(io::ErrorKind::PermissionDenied, "store is read-only"),
            });
        }
        Self::try_acquire_path(path, key)
    }

    pub fn acquire_existing(
        layout: &VirtualStoreLayout,
        key: &PackageKey,
    ) -> Result<Option<Self>, GvsSlotLockError> {
        let Some(path) = layout.hashed_slot_dir(key) else {
            return Ok(None);
        };
        if layout.frozen_store() {
            return Self::open_existing_path(path, key);
        }
        Self::acquire_existing_path(path, key)
    }

    /// Whether the slot at `path` has to be imported or built again before
    /// `key` can use it, read without the slot's lock: its package carries
    /// `.pnpm-needs-build`, or its population never finished. Writers keep
    /// both under the lock, and an import that dies leaves them behind.
    pub fn needs_recovery_at(path: &std::path::Path, key: &PackageKey) -> io::Result<bool> {
        let marker = path
            .join("node_modules")
            .join(key.name.to_string())
            .join(crate::NEEDS_BUILD_MARKER);
        if marker.is_file() {
            return Ok(true);
        }
        Self::population_unfinished_at(path)
    }

    fn population_unfinished_at(path: &std::path::Path) -> io::Result<bool> {
        match fs::read_to_string(path.join(crate::GVS_SLOT_STATE_FILE)) {
            Ok(state) => Ok(GlobalVirtualStoreSlotPhase::parse(&state)?
                == GlobalVirtualStoreSlotPhase::Populating),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(error),
        }
    }

    pub fn open_existing_path(
        path: PathBuf,
        key: &PackageKey,
    ) -> Result<Option<Self>, GvsSlotLockError> {
        if !path.is_dir() {
            return Ok(None);
        }
        Ok(Some(Self::from_path(path, key, None)))
    }

    pub fn acquire_existing_path(
        path: PathBuf,
        key: &PackageKey,
    ) -> Result<Option<Self>, GvsSlotLockError> {
        if !path.is_dir() {
            return Ok(None);
        }
        let slot = Self::acquire_path(path, key)?;
        if slot
            .is_pruned_shell()
            .map_err(|source| GvsSlotLockError { dep_path: key.to_string(), source })?
        {
            let path = slot.path.clone();
            drop(slot);
            match fs::remove_dir(path) {
                Ok(()) => {}
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::NotFound | io::ErrorKind::DirectoryNotEmpty,
                    ) => {}
                Err(source) => {
                    return Err(GvsSlotLockError { dep_path: key.to_string(), source });
                }
            }
            return Ok(None);
        }
        Ok(Some(slot))
    }

    fn from_path(path: PathBuf, key: &PackageKey, lock: Option<DirLock>) -> Self {
        let package_path = path.join("node_modules").join(key.name.to_string());
        let needs_build_marker = package_path.join(crate::NEEDS_BUILD_MARKER);
        let state_file = path.join(crate::GVS_SLOT_STATE_FILE);
        Self { path, package_path, needs_build_marker, state_file, lock }
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
        build_requirement: BuildRequirement,
    ) -> io::Result<GlobalVirtualStoreSlotState> {
        let linked_archive = self.linked_archive();
        let phase = self.phase(completion_marker, build_requirement)?;
        Ok(GlobalVirtualStoreSlotState { phase, linked_archive })
    }

    /// [`Self::state`] without the linked archive record.
    pub fn phase(
        &self,
        completion_marker: &std::path::Path,
        build_requirement: BuildRequirement,
    ) -> io::Result<GlobalVirtualStoreSlotPhase> {
        if !self.package_path.is_dir() {
            return Ok(GlobalVirtualStoreSlotPhase::Missing);
        }
        match self.read_state_file()? {
            Some(phase) => Ok(phase),
            None => self.legacy_state(completion_marker, build_requirement),
        }
    }

    fn read_state_file(&self) -> io::Result<Option<GlobalVirtualStoreSlotPhase>> {
        match fs::read_to_string(&self.state_file) {
            Ok(state) => GlobalVirtualStoreSlotPhase::parse(&state).map(Some),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        }
    }

    fn legacy_state(
        &self,
        completion_marker: &std::path::Path,
        build_requirement: BuildRequirement,
    ) -> io::Result<GlobalVirtualStoreSlotPhase> {
        match fs::metadata(completion_marker) {
            Ok(metadata) if metadata.is_file() => {
                let absent_phase = if build_requirement == BuildRequirement::Required {
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
        let phase =
            self.phase(&self.package_path.join("package.json"), BuildRequirement::NotRequired)?;
        Ok(matches!(
            phase,
            GlobalVirtualStoreSlotPhase::Populating
                | GlobalVirtualStoreSlotPhase::BuildPending
                | GlobalVirtualStoreSlotPhase::Building,
        ))
    }

    pub fn begin_population(&self) -> io::Result<()> {
        self.write_state(GlobalVirtualStoreSlotPhase::Populating)
    }

    pub fn is_populated(&self) -> io::Result<bool> {
        match self.read_state_file()? {
            Some(GlobalVirtualStoreSlotPhase::Populated | GlobalVirtualStoreSlotPhase::Built) => {
                Ok(true)
            }
            Some(_) | None => Ok(false),
        }
    }

    pub fn finish_population(&self, build_requirement: BuildRequirement) -> io::Result<()> {
        self.write_state(match build_requirement {
            BuildRequirement::Required => GlobalVirtualStoreSlotPhase::BuildPending,
            BuildRequirement::NotRequired => GlobalVirtualStoreSlotPhase::Populated,
        })
    }

    pub fn mark_build_started(&self) -> io::Result<()> {
        self.write_state(GlobalVirtualStoreSlotPhase::Building)?;
        fs::write(&self.needs_build_marker, b"started")
    }

    pub fn mark_build_pending(&self) -> io::Result<()> {
        self.write_state(GlobalVirtualStoreSlotPhase::BuildPending)?;
        fs::write(&self.needs_build_marker, b"")
    }

    pub fn mark_build_finished(&self) -> io::Result<()> {
        self.ensure_writable()?;
        match fs::remove_file(&self.needs_build_marker) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        self.write_state(GlobalVirtualStoreSlotPhase::Built)
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
        self.ensure_writable()?;
        pnpm_fs::write_atomic(
            &self.path.join(crate::create_virtual_store::linked_archives::LINKED_ARCHIVE_RECORD),
            integrity.as_bytes(),
        )
    }

    fn write_state(&self, state: GlobalVirtualStoreSlotPhase) -> io::Result<()> {
        self.ensure_writable()?;
        let state = state
            .as_str()
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidInput, "cannot persist a missing slot phase")
            })?;
        pnpm_fs::write_atomic(&self.state_file, state.as_bytes())
    }

    fn ensure_writable(&self) -> io::Result<()> {
        if self.lock.is_none() {
            return Err(io::Error::new(io::ErrorKind::PermissionDenied, "store is read-only"));
        }
        Ok(())
    }

    fn is_pruned_shell(&self) -> io::Result<bool> {
        for entry in fs::read_dir(&self.path)? {
            if entry?.file_name() != SLOT_LOCK_DIR {
                return Ok(false);
            }
        }
        Ok(true)
    }
}

#[cfg(test)]
mod tests;
