//! The importers a run installs, as a selection narrows them.

use super::{
    super::{
        HashSet,
        PackageManifest,
        Path,
        PathBuf,
    },
    InstallScope,
    RunExecution,
};

/// The importers a selection narrows the run to.
pub(super) struct ImporterSelection {
    pub(super) real_importer_ids: HashSet<String>,
    pub(super) filtered_install: bool,
    pub(super) requested_importer_ids: Option<HashSet<String>>,
}
impl ImporterSelection {
    /// A `--filter` selection requests its importers even when it matches
    /// every project on disk, so the importers of projects missing from disk
    /// stay in the lockfile.
    pub(super) fn select(
        selection: Option<&crate::WorkspaceInstallSelection<'_>>,
        has_filter: bool,
        workspace_root: &Path,
        project_manifests: &[(PathBuf, &PackageManifest)],
    ) -> Self {
        let real_importer_ids = importer_ids(
            workspace_root,
            project_manifests.iter().map(|(project_dir, _)| project_dir.as_path()),
        );
        let filtered_install = selection.is_some_and(|selection| {
            importer_ids(workspace_root, selection.selected_dirs.iter().map(PathBuf::as_path))
                != real_importer_ids
        });
        Self {
            requested_importer_ids: (filtered_install || has_filter)
                .then_some(selection)
                .flatten()
                .map(|selection| {
                    importer_ids(
                        workspace_root,
                        selection.install_dirs.iter().map(PathBuf::as_path),
                    )
                }),
            real_importer_ids,
            filtered_install,
        }
    }
}
impl RunExecution<'_> {
    pub(super) fn retain_missing_mutation_importers(
        &self,
        scope: &mut InstallScope<'_>,
        wanted: Option<&pnpm_lockfile::Lockfile>,
    ) {
        if self.options.selection.is_some()
            || !self.install.context.config.shares_one_lockfile()
            || self.workspace.dirs.workspace_dir.is_none()
            || !matches!(
                self.install.execution.mutation,
                crate::ProjectMutation::InstallSome | crate::ProjectMutation::InstallSelected,
            )
        {
            return;
        }
        let on_disk = &scope.importers.real_importer_ids;
        if wanted.is_some_and(|lockfile| {
            pnpm_deps_restorer::missing_project_importers(lockfile, on_disk).next().is_some()
        }) {
            scope.importers.requested_importer_ids = Some(on_disk.clone());
            scope.prune_stale_importers = false;
        }
    }
}
fn importer_ids<'d>(
    workspace_root: &Path,
    dirs: impl Iterator<Item = &'d Path>,
) -> HashSet<String> {
    dirs.map(|project_dir| pnpm_workspace::importer_id_from_root_dir(workspace_root, project_dir))
        .collect()
}

#[cfg(test)]
mod tests;
