use super::{
    DependencyGroup,
    HashMap,
    ImporterDepVersion,
    Lockfile,
    Path,
    ProjectPathKey,
    SelectedProject,
    lockfile::importer_package_dirs,
};

/// Linked projects have importer records rather than package snapshots. Walk
/// their included links to find a directory that needs fresh resolution.
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
    let runtime_groups: &[DependencyGroup] = if groups.contains(&DependencyGroup::Optional) {
        &[DependencyGroup::Prod, DependencyGroup::Optional]
    } else {
        &[DependencyGroup::Prod]
    };
    while let Some(&id) = pending.get_index(index) {
        index += 1;
        let root = lockfile_dir.join(id);
        let included = if id == project_id { groups } else { runtime_groups };
        for target in linked_directories(lockfile, id, included) {
            let by_dir =
                projects.get_or_init(|| importer_ids_by_dir(lockfile, selected, lockfile_dir));
            let Some(linked) = by_dir.get(&ProjectPathKey::new(&root.join(target))) else {
                return true;
            };
            pending.insert(*linked);
        }
    }
    false
}

fn linked_directories<'a>(
    lockfile: &'a Lockfile,
    id: &str,
    groups: &'a [DependencyGroup],
) -> impl Iterator<Item = &'a str> {
    lockfile.importers
        .get(id)
        .into_iter()
        .flat_map(move |snapshot| snapshot.dependencies_by_groups(groups.iter().copied()))
        .filter_map(|(_, dependency)| match &dependency.version {
            ImporterDepVersion::Link(target) => Some(target.as_str()),
            _ => None,
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
