//! `--lockfile-dir`, the install-family flag that pins where
//! `pnpm-lock.yaml` is created.
//!
//! [`LockfileDirArg`] is flattened into every install-family command that
//! accepts the flag and applied to the loaded [`Config`] through
//! [`LockfileDirArg::apply_to`] before the state is built, so the whole
//! install — the lockfile, the root `node_modules`, the virtual store, and
//! the importer ids — is anchored at the pinned directory.

use clap::Args;
use derive_more::{Display, Error};
use miette::Diagnostic;
use pnpm_config::Config;
use std::path::{Path, PathBuf};

// Doc comments on a clap-derived type reach `--help`, so the contract
// lives in the module doc above rather than in intra-doc links here.
#[derive(Debug, Default, Clone, Args)]
pub struct LockfileDirArg {
    /// The directory in which `pnpm-lock.yaml` is created. Several
    /// projects may share a single lockfile.
    #[clap(long = "lockfile-dir", value_name = "dir")]
    pub lockfile_dir: Option<PathBuf>,
}

/// A global install owns a lockfile under its own group directory, so
/// there is nothing for `--lockfile-dir` to point at. Mirrors pnpm's
/// `CONFIG_CONFLICT_LOCKFILE_DIR_WITH_GLOBAL`.
#[derive(Debug, Display, Error, Diagnostic)]
#[display(r#"Configuration conflict. "lockfile-dir" may not be used with "global""#)]
#[diagnostic(code(ERR_PNPM_CONFIG_CONFLICT_LOCKFILE_DIR_WITH_GLOBAL))]
pub struct LockfileDirWithGlobal;

impl LockfileDirArg {
    /// Pin `config`'s lockfile directory, resolving a relative flag value
    /// against `dir` — the canonicalized `--dir`, which is what pnpm
    /// resolves its own `lockfileDir` against.
    pub(crate) fn apply_to(&self, config: &mut Config, dir: &Path) {
        if let Some(lockfile_dir) = self.lockfile_dir.as_deref() {
            config.pin_lockfile_dir(&dir.join(lockfile_dir));
        }
    }

    /// The `--global` counterpart of [`Self::apply_to`]: the flag is an
    /// error there, and a `lockfileDir` that reached the config from
    /// another source is dropped — pnpm deletes the setting rather than
    /// erroring when the CLI did not ask for it.
    pub(crate) fn apply_to_global(&self, config: &mut Config) -> Result<(), LockfileDirWithGlobal> {
        if self.lockfile_dir.is_some() {
            return Err(LockfileDirWithGlobal);
        }
        config.lockfile_dir = None;
        Ok(())
    }
}
