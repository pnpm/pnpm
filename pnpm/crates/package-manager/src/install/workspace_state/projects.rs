use super::super::{DependencyGroup, HashSet, PackageManifest, Path, PathBuf, ProjectMutation};

pub(in super::super) fn build_project_manifests_list<'a>(
    root_manifest: &'a PackageManifest,
    workspace_projects: Option<&'a [pnpm_workspace::Project]>,
) -> Vec<(std::path::PathBuf, &'a PackageManifest)> {
    let active_dir = root_manifest.path().parent().expect("manifest path always has a parent dir");
    let Some(projects) = workspace_projects else {
        return vec![(active_dir.to_path_buf(), root_manifest)];
    };
    let active_dir_matcher = ProjectDirMatcher::new(active_dir);
    let mut active_project_was_discovered = false;
    let mut list = projects
        .iter()
        .map(|project| {
            if active_dir_matcher.matches(&project.root_dir) {
                active_project_was_discovered = true;
                (active_dir.to_path_buf(), root_manifest)
            } else {
                (project.root_dir.clone(), &project.manifest)
            }
        })
        .collect::<Vec<_>>();
    let active_manifest_has_dependencies = root_manifest
        .dependencies([
            DependencyGroup::Prod,
            DependencyGroup::Dev,
            DependencyGroup::Optional,
            DependencyGroup::Peer,
        ])
        .next()
        .is_some();
    if !active_project_was_discovered
        && (root_manifest.path().is_file() || active_manifest_has_dependencies)
    {
        list.push((active_dir.to_path_buf(), root_manifest));
    }
    list
}
pub(in super::super) fn build_root_importer_project_manifests_list<'a>(
    workspace_root: &Path,
    root_manifest: &'a PackageManifest,
    workspace_projects: Option<&'a [pnpm_workspace::Project]>,
) -> Vec<(PathBuf, &'a PackageManifest)> {
    let mut list = vec![(workspace_root.to_path_buf(), root_manifest)];
    if let Some(projects) = workspace_projects {
        let workspace_root_matcher = ProjectDirMatcher::new(workspace_root);
        list.extend(
            projects
                .iter()
                .filter(|project| !workspace_root_matcher.matches(&project.root_dir))
                .map(|project| (project.root_dir.clone(), &project.manifest)),
        );
    }
    list
}
pub(in super::super) fn build_selected_project_manifests_list<'a>(
    active_manifest: &'a PackageManifest,
    projects: &'a [pnpm_workspace::Project],
    active_manifest_is_standin: bool,
) -> Vec<(PathBuf, &'a PackageManifest)> {
    let mut manifests = projects
        .iter()
        .map(|project| (project.root_dir.clone(), &project.manifest))
        .collect::<Vec<_>>();
    let active_dir =
        active_manifest.path().parent().expect("manifest path always has a parent dir");
    let active_dir_matcher = ProjectDirMatcher::new(active_dir);
    let active_project_was_discovered =
        projects.iter().any(|project| active_dir_matcher.matches(&project.root_dir));
    if !active_manifest_is_standin && !active_project_was_discovered {
        manifests.push((active_dir.to_path_buf(), active_manifest));
    }
    manifests
}
/// Matches workspace project roots against one fixed directory.
///
/// Two paths can name the same project without being equal as strings —
/// a symlinked workspace root, or `/tmp` against its `/private/tmp` target
/// on macOS — so a lexical mismatch falls back to comparing canonical
/// paths. Canonicalizing touches the filesystem once per candidate, so the
/// fixed side is resolved up front rather than once per project.
pub(in super::super) struct ProjectDirMatcher {
    normalized: PathBuf,
    canonical: Option<PathBuf>,
}
impl ProjectDirMatcher {
    fn new(dir: &Path) -> Self {
        let normalized = pnpm_fs::lexical_normalize(dir);
        let canonical = std::fs::canonicalize(&normalized).ok();
        ProjectDirMatcher { normalized, canonical }
    }

    fn matches(&self, project_dir: &Path) -> bool {
        let project_dir = pnpm_fs::lexical_normalize(project_dir);
        if project_dir == self.normalized {
            return true;
        }
        let Some(canonical) = self.canonical.as_deref() else {
            return false;
        };
        std::fs::canonicalize(project_dir).is_ok_and(|project_dir| project_dir == canonical)
    }
}
pub(in super::super) struct ProjectScriptsInputs<'a, 'manifest> {
    pub(in super::super) mutation: ProjectMutation,
    pub(in super::super) workspace_root: &'a Path,
    /// The project the command was run in, which is the sole mutated
    /// importer of an unfiltered `add` / `update`.
    pub(in super::super) active_project_dir: &'a Path,
    /// The `--filter` / `-r` selection, when the run was narrowed to one.
    pub(in super::super) selected_dirs: Option<&'a HashSet<PathBuf>>,
    /// Every project of the workspace.
    pub(in super::super) project_manifests: &'a [(PathBuf, &'manifest PackageManifest)],
    /// The subset this run materialized. A project outside it has no
    /// linked `node_modules` for its scripts to import, so it never runs
    /// them even when the mutated set nominally covers it.
    pub(in super::super) materialized_project_manifests:
        &'a [(PathBuf, &'manifest PackageManifest)],
}
/// The projects that fire their own lifecycle scripts at the end of this
/// run, following pnpm's mutated-importer rule (see [`ProjectMutation`]).
pub(in super::super) fn projects_running_own_scripts<'manifest>(
    inputs: &ProjectScriptsInputs<'_, 'manifest>,
) -> Vec<(PathBuf, &'manifest PackageManifest)> {
    let full_install = match inputs.mutation {
        ProjectMutation::NoInstall | ProjectMutation::UninstallSome => return Vec::new(),
        ProjectMutation::InstallWorkspace => return inputs.materialized_project_manifests.to_vec(),
        ProjectMutation::InstallSelected => true,
        ProjectMutation::InstallSome => false,
    };
    let mutated_dirs = match inputs.selected_dirs {
        Some(selected_dirs) => {
            selected_dirs.iter().map(|dir| pnpm_fs::lexical_normalize(dir)).collect()
        }
        None => HashSet::from([pnpm_fs::lexical_normalize(inputs.active_project_dir)]),
    };
    // pnpm's recursive dispatch pushes the workspace root into the
    // mutated importers as a plain `mutation: 'install'` whenever the
    // selection leaves it out, so the root installs in full — and runs
    // its own scripts — even when the command was pointed elsewhere.
    let workspace_root = pnpm_fs::lexical_normalize(inputs.workspace_root);
    let root_was_pushed_in = !mutated_dirs.contains(&workspace_root);
    let is_pushed_root = |project_dir: &Path| root_was_pushed_in && project_dir == workspace_root;
    // A run that mutates only part of the workspace materializes the rest
    // from the lockfile alone; pnpm runs the scripts of everything it did
    // mutate, whatever the inputs.mutation. Only when the mutated set covers the
    // whole workspace does the `inputs.mutation === 'install'` filter decide.
    let covers_workspace = inputs.project_manifests.iter().all(|(project_dir, _)| {
        let project_dir = pnpm_fs::lexical_normalize(project_dir);
        mutated_dirs.contains(&project_dir) || is_pushed_root(&project_dir)
    });
    inputs
        .materialized_project_manifests
        .iter()
        .filter(|(project_dir, _)| {
            let project_dir = pnpm_fs::lexical_normalize(project_dir);
            let pushed_root = is_pushed_root(&project_dir);
            if !pushed_root && !mutated_dirs.contains(&project_dir) {
                return false;
            }
            !covers_workspace || full_install || pushed_root
        })
        .cloned()
        .collect()
}
pub(in super::super) fn selected_manifest_freshness_inputs<'a>(
    workspace_root: &Path,
    project_manifests: &[(PathBuf, &'a PackageManifest)],
    selected_dirs: &HashSet<PathBuf>,
) -> Vec<(String, &'a PackageManifest)> {
    let selected_dirs =
        selected_dirs.iter().map(|dir| pnpm_fs::lexical_normalize(dir)).collect::<HashSet<_>>();
    let mut inputs = project_manifests
        .iter()
        .filter(|(project_dir, _)| selected_dirs.contains(&pnpm_fs::lexical_normalize(project_dir)))
        .map(|(project_dir, manifest)| {
            (pnpm_workspace::importer_id_from_root_dir(workspace_root, project_dir), *manifest)
        })
        .collect::<Vec<_>>();
    inputs.sort_by(|(left, _), (right, _)| left.cmp(right));
    inputs
}
