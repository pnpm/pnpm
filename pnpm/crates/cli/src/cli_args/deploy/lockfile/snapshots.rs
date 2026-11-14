use super::{ConvertCtx, importer_package_dirs};
use crate::cli_args::deploy::{
    HashMap, LinkedWorkspaceProject, Lockfile, Path, PkgNameVerPeer, ProjectSnapshot, ResolveBases,
    SelectedProject, SnapshotEntry, convert_snapshot, create_file_url_key, lexical_normalize,
    project_snapshot_to_snapshot_entry,
};

pub(super) struct DeploySnapshots {
    pub(super) snapshots: HashMap<PkgNameVerPeer, SnapshotEntry>,
    pub(super) linked_workspace_projects: HashMap<PkgNameVerPeer, LinkedWorkspaceProject>,
}

pub(super) fn convert_deploy_snapshots(
    lockfile: &Lockfile,
    project_id: &str,
    lockfile_dir: &Path,
    selected: &SelectedProject,
    ctx: &ConvertCtx<'_>,
) -> miette::Result<DeploySnapshots> {
    let mut converted =
        DeploySnapshots { snapshots: HashMap::new(), linked_workspace_projects: HashMap::new() };
    for (key, snapshot) in lockfile.snapshots.iter().flatten() {
        let output_key = super::convert_package_key(key, ctx)?;
        converted.snapshots.insert(output_key, convert_snapshot(snapshot, ctx, lockfile_dir)?);
    }
    for (importer_path, project_snapshot) in &lockfile.importers {
        if importer_path == project_id {
            continue;
        }
        let project_root = lexical_normalize(&lockfile_dir.join(importer_path));
        converted.insert_importer(lockfile, (&project_root, project_snapshot), selected, ctx)?;
    }
    Ok(converted)
}

impl DeploySnapshots {
    fn insert_importer(
        &mut self,
        lockfile: &Lockfile,
        (project_root, project_snapshot): (&Path, &ProjectSnapshot),
        selected: &SelectedProject,
        ctx: &ConvertCtx<'_>,
    ) -> miette::Result<()> {
        let bases = ResolveBases { file_base: ctx.lockfile_dir, link_base: project_root };
        let snapshot = project_snapshot_to_snapshot_entry(project_snapshot, ctx, &bases)?;
        for package_dir in
            importer_package_dirs(project_root, project_snapshot, &selected.projects_by_path)
        {
            let package_key =
                create_file_url_key(&package_dir, "", &selected.projects_by_path, None)?;
            // An injected snapshot records its own peer context, which can differ from the importer.
            let std::collections::hash_map::Entry::Vacant(entry) = self
                .snapshots
                .entry(package_key.clone())
            else {
                continue;
            };
            // Own dependencies declared as peers still need manifest-based edge classification.
            if let Some(project) = selected.project_info(project_root)
                && !project.peer_set.declared().is_empty()
            {
                self.linked_workspace_projects.insert(
                    package_key,
                    LinkedWorkspaceProject::new(
                        project.clone(),
                        lockfile,
                        project_snapshot,
                        ctx,
                        &bases,
                    ),
                );
            }
            entry.insert(snapshot.clone());
        }
        Ok(())
    }
}
