//! Cargo packages in the store, at `<store_dir>/crates/<name>/<version>/<slot>`.
//!
//! - A Cargo workspace links the slots it uses from its
//!   [`CARGO_REGISTRY_LINK_DIR`] and [`CARGO_GIT_LINK_DIR`].
//! - An install registers the workspace in [`StoreDir::cargo_workspaces`], a
//!   registry of its own: the npm project registry's mark walks every
//!   directory of a project, and a Cargo `target` directory is large.
//! - `store prune` removes a slot that no registered workspace links, but only
//!   a slot that has a linked record (see [`record_linked_crate_slot`]).

use crate::{
    RegisterProjectError,
    StoreDir,
    project_registry::register_in,
};
use std::{
    ffi::OsString,
    fs,
    io,
    path::{
        Path,
        PathBuf,
    },
};

/// The directory, relative to a Cargo workspace root, that links the
/// registry crates the workspace uses.
pub const CARGO_REGISTRY_LINK_DIR: [&str; 3] = [".pnpm", "crates", "crates-io"];

/// The directory, relative to a Cargo workspace root, that links the git
/// packages the workspace uses.
pub const CARGO_GIT_LINK_DIR: [&str; 3] = [".pnpm", "crates", "git"];

/// The file name suffix of a slot's linked record, which sits beside the slot.
const LINKED_RECORD_SUFFIX: &str = ".linked";

impl StoreDir {
    /// The directory of the Cargo package slots.
    pub fn crates(&self) -> PathBuf {
        self.root().join("crates")
    }

    /// The registry of the Cargo workspaces installed from the store.
    pub fn cargo_workspaces(&self) -> PathBuf {
        self.root().join("cargo-workspaces")
    }
}

/// Register the Cargo workspace at `root` in [`StoreDir::cargo_workspaces`].
pub fn register_cargo_workspace(
    store_dir: &StoreDir,
    root: &Path,
) -> Result<(), RegisterProjectError> {
    register_in(&store_dir.cargo_workspaces(), root)
}

/// Record that a workspace registered in the store links `slot`.
///
/// An older pnpm linked slots without registering the workspace, so a slot
/// without this record may still be in use by a workspace `store prune`
/// cannot find. `store prune` keeps such a slot.
pub fn record_linked_crate_slot(slot: &Path) -> io::Result<()> {
    let record = linked_record_path(slot);
    match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&record)
    {
        Ok(_) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Ok(()),
        Err(error) => Err(error),
    }
}

fn linked_record_path(slot: &Path) -> PathBuf {
    let mut name = slot
        .file_name()
        .map(OsString::from)
        .unwrap_or_default();
    name.push(LINKED_RECORD_SUFFIX);
    slot.with_file_name(name)
}

/// The slot whose linked record is at `record`, or `None` when `record` is
/// not one.
pub(crate) fn slot_of_linked_record(record: &Path) -> Option<PathBuf> {
    let name = record.file_name()?.to_str()?;
    let slot = name.strip_suffix(LINKED_RECORD_SUFFIX)?;
    Some(record.with_file_name(slot))
}
