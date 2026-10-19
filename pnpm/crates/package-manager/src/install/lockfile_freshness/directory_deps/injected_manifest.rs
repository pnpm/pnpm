use pnpm_package_manifest::PackageManifest;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

/// Project manifests keyed by their lexically normalized project directory.
pub type ProjectManifestsByDir<'a> = HashMap<PathBuf, &'a PackageManifest>;

/// Each project's manifest as another importer's injected dependency reads
/// it: the project's entry in `dependency_manifests` when it has one, else
/// its importer manifest.
pub(crate) fn project_manifests_by_dir<'a>(
    manifests: impl IntoIterator<Item = &'a PackageManifest>,
    dependency_manifests: Option<&ProjectManifestsByDir<'a>>,
) -> ProjectManifestsByDir<'a> {
    let mut by_dir: ProjectManifestsByDir<'a> = manifests
        .into_iter()
        .filter_map(|manifest| {
            let dir = manifest.path().parent()?;
            Some((pnpm_fs::lexical_normalize(dir), manifest))
        })
        .collect();
    if let Some(dependency_manifests) = dependency_manifests {
        by_dir.extend(
            dependency_manifests
                .iter()
                .map(|(dir, manifest)| (dir.clone(), *manifest)),
        );
    }
    by_dir
}

/// The dependency manifests `projects` carry (see
/// [`pnpm_workspace::Project::dependency_manifest`]), keyed by project
/// directory. `None` when no project carries one, which is every install the
/// Node-API binding did not hand such a manifest to.
pub(crate) fn dependency_manifests_by_dir(
    projects: Option<&[pnpm_workspace::Project]>,
) -> Option<ProjectManifestsByDir<'_>> {
    let by_dir: ProjectManifestsByDir<'_> = projects?
        .iter()
        .filter_map(|project| {
            let manifest = project.dependency_manifest.as_ref()?;
            Some((pnpm_fs::lexical_normalize(&project.root_dir), manifest))
        })
        .collect();
    (!by_dir.is_empty()).then_some(by_dir)
}

/// A local directory dependency's directory and where its manifest may come
/// from.
pub(in crate::install::lockfile_freshness) struct InjectedDirectory<'a> {
    pub(in crate::install::lockfile_freshness) dir: &'a Path,
    /// See [`super::LocalDepContext::declared_as_file`].
    pub(in crate::install::lockfile_freshness) declared_as_file: bool,
    pub(in crate::install::lockfile_freshness) workspace_root: &'a Path,
    pub(in crate::install::lockfile_freshness) manifests_by_dir: &'a ProjectManifestsByDir<'a>,
}

/// The manifest the resolver described the dependency with, before any hook.
/// A workspace project injected as its `publishConfig.directory` is
/// described by the project's own manifest, not by a `package.json` a build
/// wrote into that directory. A `file:` dependency is described by the
/// directory's `package.json`, and by the publishing project's only while
/// that is missing.
pub(in crate::install::lockfile_freshness) fn unhooked_manifest(
    injected: &InjectedDirectory<'_>,
) -> Option<PackageManifest> {
    let project_manifest = || {
        injected.manifests_by_dir
            .get(&pnpm_fs::lexical_normalize(injected.dir))
            .map(|manifest| (*manifest).clone())
    };
    let on_disk = || pnpm_workspace::safe_read_project_manifest_only(injected.dir).ok().flatten();
    let publishing_project = || manifest_of_project_publishing_from(injected);
    let manifest = if injected.declared_as_file {
        project_manifest().or_else(on_disk).or_else(publishing_project)
    } else {
        project_manifest().or_else(publishing_project).or_else(on_disk)
    };
    manifest.or_else(|| directory_name_manifest(injected.dir))
}

/// The manifest the resolver gives an existing directory that nothing else
/// describes: its name alone. A manifest that fails to read describes it
/// too, so it gets none.
fn directory_name_manifest(dir: &Path) -> Option<PackageManifest> {
    if !dir.is_dir() || !matches!(pnpm_workspace::safe_read_project_manifest_only(dir), Ok(None)) {
        return None;
    }
    let name = dir
        .file_name()?
        .to_string_lossy()
        .into_owned();
    Some(PackageManifest::from_value(dir.join("package.json"), serde_json::json!({ "name": name })))
}

/// The manifest of the project whose `publishConfig.directory` is
/// `injected.dir`: the project's entry in `manifests_by_dir` when it has one,
/// else its `package.json`. Workspace dependencies only consider projects
/// within the workspace root; `file:` dependencies walk every ancestor,
/// matching [`pnpm_package_manifest::find_parent_publish_manifest`].
fn manifest_of_project_publishing_from(
    injected: &InjectedDirectory<'_>,
) -> Option<PackageManifest> {
    let target = pnpm_fs::lexical_normalize(injected.dir);
    let workspace_root = pnpm_fs::lexical_normalize(injected.workspace_root);
    if !injected.declared_as_file && !target.starts_with(&workspace_root) {
        return None;
    }
    for candidate in target.ancestors().skip(1) {
        if !injected.declared_as_file && !candidate.starts_with(&workspace_root) {
            return None;
        }
        let manifest = injected.manifests_by_dir
            .get(candidate)
            .map(|manifest| (*manifest).clone())
            .or_else(|| pnpm_workspace::safe_read_project_manifest_only(candidate).ok().flatten());
        if let Some(manifest) = manifest
            && pnpm_package_manifest::publishes_from_directory(candidate, manifest.value(), &target)
        {
            return Some(manifest);
        }
    }
    None
}
