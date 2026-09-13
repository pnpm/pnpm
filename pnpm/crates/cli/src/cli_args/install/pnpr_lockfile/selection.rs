use super::{
    DependencyGroup, IncludedDependencies, InstallFamilySelection, LocalLockfileInstall, Lockfile,
    NodeLinker, PnprLink, ResolveProject, SkippedSnapshots, State, materialization_closure,
};

/// The importer ids of the selected projects, as `(every project of the
/// selection, the ones being installed)`.
///
/// Importer ids name projects relative to the lockfile, which
/// `lockfileDir` can pin somewhere other than the workspace the selection
/// was resolved in. The server request, the merge, and the lockfile on
/// disk all have to agree on them.
pub(in super::super) fn selection_importer_ids(
    state: &State,
    selection: Option<&InstallFamilySelection>,
) -> Option<(
    std::collections::HashSet<String>,
    std::collections::HashSet<String>,
)> {
    let selection = selection?;
    let importer_root = state.config.lockfile_dir_for(&selection.workspace_root);
    let real_importer_ids = selection.projects
        .iter()
        .map(|project| pnpm_workspace::importer_id_from_root_dir(importer_root, &project.root_dir))
        .collect();
    let selected_importer_ids = selection.install_dirs
        .iter()
        .map(|project_dir| pnpm_workspace::importer_id_from_root_dir(importer_root, project_dir))
        .collect();
    Some((real_importer_ids, selected_importer_ids))
}

/// A workspace-wide install has no selection, but its merge still has to
/// name every importer the one shared lockfile covers.
pub(in super::super) fn full_workspace_importer_ids(
    state: &State,
    selection: Option<&InstallFamilySelection>,
    link: &PnprLink<'_>,
    projects: &[ResolveProject],
) -> Option<(
    std::collections::HashSet<String>,
    std::collections::HashSet<String>,
)> {
    if selection.is_some()
        || !link.use_state_lockfile
        || !state.config.shares_one_lockfile()
        || state.config.workspace_dir.is_none()
    {
        return None;
    }
    let importer_ids: std::collections::HashSet<_> = projects
        .iter()
        .map(|project| project.dir.clone())
        .collect();
    Some((importer_ids.clone(), importer_ids))
}

pub(super) fn selected_prefetch_lockfile(
    link: &PnprLink<'_>,
    local: &LocalLockfileInstall<'_>,
) -> Option<Lockfile> {
    local.selection_importer_ids.map(|(_, selected_importer_ids)| {
        let hoisted_importer_ids = matches!(link.node_linker, NodeLinker::Hoisted).then(|| {
            local.lockfile.importers
                .keys()
                .cloned()
                .collect::<std::collections::HashSet<_>>()
        });
        let initial_importer_ids = hoisted_importer_ids.as_ref().unwrap_or(selected_importer_ids);
        materialization_closure(
            local.lockfile,
            local.lockfile_dir,
            initial_importer_ids,
            IncludedDependencies {
                dependencies: link.dependency_groups.contains(&DependencyGroup::Prod),
                dev_dependencies: link.dependency_groups.contains(&DependencyGroup::Dev),
                optional_dependencies: link.dependency_groups.contains(&DependencyGroup::Optional),
            },
            &SkippedSnapshots::new(),
        )
        .lockfile
    })
}
