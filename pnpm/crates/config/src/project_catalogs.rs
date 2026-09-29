//! The catalogs of a workspace project that keeps its own lockfile and its
//! own `pnpm-workspace.yaml`.
//!
//! Such a project is usually one that is also installed on its own, such as
//! a git submodule: installed there, its `pnpm-workspace.yaml` is the
//! workspace root and its catalogs are the ones `catalog:` resolves against.
//! Resolving it against the same catalogs when the outer workspace installs
//! it keeps the two installs writing the same lockfile.

use crate::Config;
use derive_more::{Display, Error};
use miette::Diagnostic;
use pnpm_catalogs_config::{
    InvalidCatalogsConfigurationError, get_catalogs_from_workspace_manifest,
};
use pnpm_catalogs_types::Catalogs;
use pnpm_workspace::{ReadWorkspaceManifestError, read_workspace_manifest};
use std::path::Path;

/// A project's own `pnpm-workspace.yaml` whose catalogs cannot be read.
#[derive(Debug, Display, Error, Diagnostic)]
pub enum ProjectCatalogsError {
    #[diagnostic(transparent)]
    ReadWorkspaceManifest(#[error(source)] ReadWorkspaceManifestError),

    #[diagnostic(transparent)]
    InvalidCatalogs(#[error(source)] InvalidCatalogsConfigurationError),
}

impl Config {
    /// Resolve `catalog:` for the project in `project_dir` against the
    /// catalogs of its own `pnpm-workspace.yaml`, the ones it inherits
    /// through `extends` included, in place of the workspace's. Nothing
    /// changes for the workspace root itself, for a project without a
    /// manifest of its own, or when the workspace shares one lockfile,
    /// where every project has to agree on each catalog entry.
    ///
    /// The project's catalogs replace, rather than extend, the workspace's,
    /// so that the project resolves exactly as it does when it is
    /// installed on its own. They replace an `updateConfig` hook's too: the
    /// hook ran on the workspace's catalogs.
    pub fn adopt_project_catalogs(
        &mut self,
        project_dir: &Path,
    ) -> Result<(), ProjectCatalogsError> {
        if let Some(catalogs) = self.own_project_catalogs(project_dir)? {
            self.catalogs = Some(catalogs);
            self.project_catalogs_dir = Some(project_dir.to_path_buf());
        }
        Ok(())
    }

    /// The catalogs [`Self::adopt_project_catalogs`] gives the project in
    /// `project_dir`, or `None` when the workspace's apply to it.
    pub fn own_project_catalogs(
        &self,
        project_dir: &Path,
    ) -> Result<Option<Catalogs>, ProjectCatalogsError> {
        if self.shares_one_lockfile() || self.is_workspace_dir(project_dir) {
            return Ok(None);
        }
        let Some(manifest) = read_workspace_manifest(project_dir)
            .map_err(ProjectCatalogsError::ReadWorkspaceManifest)?
        else {
            return Ok(None);
        };
        get_catalogs_from_workspace_manifest(Some(&manifest))
            .map(Some)
            .map_err(ProjectCatalogsError::InvalidCatalogs)
    }

    /// The directory of the `pnpm-workspace.yaml` that declares the
    /// catalogs in force: the project's own under
    /// [`Self::adopt_project_catalogs`], else the workspace's. Local paths
    /// in catalog entries are relative to it, and catalog changes are
    /// written to it.
    #[must_use]
    pub fn catalogs_dir(&self) -> Option<&Path> {
        self.project_catalogs_dir.as_deref().or(self.workspace_dir.as_deref())
    }

    fn is_workspace_dir(&self, dir: &Path) -> bool {
        self.workspace_dir
            .as_deref()
            .is_some_and(|workspace_dir| {
                pnpm_fs::lexical_normalize(workspace_dir) == pnpm_fs::lexical_normalize(dir)
            })
    }
}

#[cfg(test)]
mod tests;
