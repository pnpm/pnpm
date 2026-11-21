use super::{
    DependencyGroup,
    HashMap,
    HashSet,
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
    let mut pending = vec![project_id];
    let mut seen = HashSet::new();
    while let Some(id) = pending.pop() {
        if !seen.insert(id) {
            continue;
        }
        let Some(snapshot) = lockfile.importers.get(id) else { continue };
        let root = lockfile_dir.join(id);
        let included = [DependencyGroup::Prod, DependencyGroup::Dev, DependencyGroup::Optional]
            .into_iter()
            .filter(|group| {
                if id == project_id {
                    return groups.contains(group);
                }
                *group == DependencyGroup::Prod
                    || (*group == DependencyGroup::Optional && groups.contains(group))
            });
        for (_, dependency) in snapshot.dependencies_by_groups(included) {
            let super::ImporterDepVersion::Link(target) = &dependency.version else { continue };
            let by_dir =
                projects.get_or_init(|| importer_ids_by_dir(lockfile, selected, lockfile_dir));
            let Some(linked) = by_dir.get(&ProjectPathKey::new(&root.join(target))) else {
                return true;
            };
            pending.push(*linked);
        }
    }
    false
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
