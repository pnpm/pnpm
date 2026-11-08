use crate::{
    ImportIndexedDirError, SymlinkPackageError, safe_join_modules_dir::InvalidDependencyAliasError,
};
use derive_more::{Display, Error};
use miette::Diagnostic;
use std::{io, path::PathBuf};

/// Error type of [`CreateVirtualDirBySnapshot`](super::CreateVirtualDirBySnapshot).
#[derive(Debug, Display, Error, Diagnostic)]
pub enum CreateVirtualDirError {
    #[display("Failed to create node_modules directory at {dir:?}: {error}")]
    #[diagnostic(code(ERR_PNPM_PACKAGE_MANAGER_CREATE_NODE_MODULES_DIR))]
    CreateNodeModulesDir {
        dir: PathBuf,
        #[error(source)]
        error: io::Error,
    },

    #[display("Failed to create virtual store slot directory at {dir:?}: {error}")]
    #[diagnostic(code(ERR_PNPM_PACKAGE_MANAGER_CREATE_SLOT_DIR))]
    CreateSlotDir {
        dir: PathBuf,
        #[error(source)]
        error: io::Error,
    },

    #[diagnostic(transparent)]
    ImportIndexedDir(#[error(source)] ImportIndexedDirError),

    #[diagnostic(transparent)]
    SlotLock(#[error(source)] crate::GvsSlotLockError),

    #[display("Failed to read global virtual store slot state at {path:?}: {source}")]
    #[diagnostic(code(ERR_PNPM_PACKAGE_MANAGER_READ_GVS_SLOT_STATE))]
    ReadSlotState {
        path: PathBuf,
        #[error(source)]
        source: io::Error,
    },

    #[display("Failed to read the build marker at {path:?}: {source}")]
    #[diagnostic(code(ERR_PNPM_PACKAGE_MANAGER_READ_BUILD_MARKER))]
    ReadBuildMarker {
        path: PathBuf,
        #[error(source)]
        source: io::Error,
    },

    #[display("Failed to update global virtual store slot state at {path:?}: {source}")]
    #[diagnostic(code(ERR_PNPM_PACKAGE_MANAGER_WRITE_GVS_SLOT_STATE))]
    WriteSlotState {
        path: PathBuf,
        #[error(source)]
        source: io::Error,
    },

    #[diagnostic(transparent)]
    SymlinkPackage(#[error(source)] SymlinkPackageError),

    /// The snapshot's package name is not a valid npm package name, so
    /// joining it under the slot's `node_modules` could escape the
    /// directory. Surfaces pnpm's `ERR_PNPM_INVALID_DEPENDENCY_NAME`.
    #[diagnostic(transparent)]
    InvalidAlias(#[error(source)] InvalidDependencyAliasError),

    #[display("Failed to remove obsolete child link at {path:?}: {error}")]
    #[diagnostic(code(ERR_PNPM_PACKAGE_MANAGER_REMOVE_OBSOLETE_CHILD))]
    RemoveObsoleteChild {
        path: PathBuf,
        #[error(source)]
        error: io::Error,
    },
}
