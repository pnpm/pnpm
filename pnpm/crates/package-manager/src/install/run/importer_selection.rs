//! The importers a run installs, as a selection narrows them.

use super::super::{HashSet, PackageManifest, Path, PathBuf};

/// The importers a selection narrows the run to.
pub(super) struct ImporterSelection {
    pub(super) real_importer_ids: HashSet<String>,
    pub(super) filtered_install: bool,
    pub(super) requested_importer_ids: Option<HashSet<String>>,
}
impl ImporterSelection {
    pub(super) fn select(
        selection: Option<&crate::WorkspaceInstallSelection<'_>>,
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
            requested_importer_ids: filtered_install
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
fn importer_ids<'d>(
    workspace_root: &Path,
    dirs: impl Iterator<Item = &'d Path>,
) -> HashSet<String> {
    dirs.map(|project_dir| pnpm_workspace::importer_id_from_root_dir(workspace_root, project_dir))
        .collect()
}
