use super::{
    DependencyGroup,
    HashMap,
    ImporterDepVersion,
    Lockfile,
    Path,
    ProjectPathKey,
    ResolveBases,
    SelectedProject,
    lockfile::importer_package_dirs,
    resolution::resolve_importer_dep_version,
};

/// Follow linked and injected workspace projects to find a linked directory
/// whose dependencies need fresh resolution.
pub(super) fn has_external_link(
    lockfile: &Lockfile,
    selected: &SelectedProject,
    lockfile_dir: &Path,
    project_id: &str,
    groups: &[DependencyGroup],
) -> bool {
    let projects = std::cell::OnceCell::new();
    let mut pending = indexmap::IndexSet::from([project_id]);
    let mut index = 0;
    while let Some(&id) = pending.get_index(index) {
        index += 1;
        let root = lockfile_dir.join(id);
        let included = if id == project_id { groups } else { runtime_groups(groups) };
        let bases = ResolveBases { link_base: &root, file_base: lockfile_dir };
        let external = local_dependencies(lockfile, id, included, &bases)
            .any(|(target, is_link)| {
                let by_dir =
                    projects.get_or_init(|| importer_ids_by_dir(lockfile, selected, lockfile_dir));
                if let Some(id) = by_dir.get(&ProjectPathKey::new(&target)) {
                    pending.insert(*id);
                    return false;
                }
                is_link
            });
        if external {
            return true;
        }
    }
    false
}

fn runtime_groups(included: &[DependencyGroup]) -> &'static [DependencyGroup] {
    if included.contains(&DependencyGroup::Optional) {
        &[DependencyGroup::Prod, DependencyGroup::Optional]
    } else {
        &[DependencyGroup::Prod]
    }
}

fn local_dependencies<'a>(
    lockfile: &'a Lockfile,
    id: &str,
    groups: &'a [DependencyGroup],
    bases: &'a ResolveBases<'a>,
) -> impl Iterator<Item = (std::path::PathBuf, bool)> + 'a {
    lockfile.importers
        .get(id)
        .into_iter()
        .flat_map(move |snapshot| snapshot.dependencies_by_groups(groups.iter().copied()))
        .filter_map(move |(_, dependency)| {
            let local = resolve_importer_dep_version(&dependency.version, bases)?;
            Some((local.resolved_path, matches!(dependency.version, ImporterDepVersion::Link(_))))
        })
}

fn importer_ids_by_dir<'a>(
    lockfile: &'a Lockfile,
    selected: &SelectedProject,
    lockfile_dir: &Path,
) -> HashMap<ProjectPathKey, &'a str> {
    let mut ids = HashMap::new();
    for (id, snapshot) in &lockfile.importers {
        let root = lockfile_dir.join(id);
        for dir in importer_package_dirs(&root, snapshot, &selected.projects_by_path) {
            ids.insert(ProjectPathKey::new(&dir), id.as_str());
        }
    }
    ids
}
