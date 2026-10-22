use pnpm_package_manifest::PackageManifest;
use std::{
    borrow::Cow,
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

pub(super) fn workspace_path_matches_target(
    dirs: &super::spec::SpecDirs<'_>,
    resolved_spec: &Path,
    resolved_target: &Path,
) -> bool {
    if resolved_spec == resolved_target {
        return true;
    }
    project_or_disk_manifest(dirs, resolved_spec)
        .is_some_and(|manifest| {
            pnpm_package_manifest::publishes_from_directory(
                resolved_spec,
                &manifest,
                resolved_target,
            )
        })
}

/// The manifest of the workspace project at `target_dir` or publishing from
/// `target_dir`: the one already loaded for it, else the ancestor project
/// whose `publishConfig.directory` is `target_dir`, else its `package.json`,
/// read only when it stays inside the workspace.
pub(super) fn target_manifest<'a>(
    dirs: &super::spec::SpecDirs<'a>,
    target_dir: &Path,
) -> Option<Cow<'a, serde_json::Value>> {
    let normalized = pnpm_fs::lexical_normalize(target_dir);
    if dirs.manifests_by_dir.contains_key(&normalized) {
        return project_or_disk_manifest(dirs, target_dir);
    }
    publishing_project_manifest(dirs, &normalized)
        .or_else(|| project_or_disk_manifest(dirs, target_dir))
}

fn project_or_disk_manifest<'a>(
    dirs: &super::spec::SpecDirs<'a>,
    target_dir: &Path,
) -> Option<Cow<'a, serde_json::Value>> {
    let normalized = pnpm_fs::lexical_normalize(target_dir);
    let manifest_path = target_dir.join("package.json");
    if let Some(manifest) = dirs.manifests_by_dir.get(&normalized) {
        let canonical_root = loaded_project_boundary(dirs, &normalized, target_dir)?;
        // A project passed in memory may have no `package.json`. One that has
        // a `package.json` linking outside its boundary is not trusted, as the
        // disk read below does not trust it.
        let escapes = std::fs::symlink_metadata(&manifest_path).is_ok()
            && !dirs.canonical_paths
                .canonicalize(&manifest_path)
                .is_ok_and(|canonical| pnpm_fs::is_subdir(&canonical_root, &canonical));
        return (!escapes).then_some(Cow::Borrowed(manifest.value()));
    }
    let canonical_root =
        target_dir_within_workspace(dirs.canonical_paths, dirs.workspace_root, target_dir).ok()?;
    let canonical_manifest = dirs.canonical_paths.canonicalize(&manifest_path).ok()?;
    if !pnpm_fs::is_subdir(&canonical_root, &canonical_manifest) {
        return None;
    }
    let content = std::fs::read_to_string(&canonical_manifest).ok()?;
    pnpm_package_manifest::parse_manifest(&content).ok().map(Cow::Owned)
}

fn loaded_project_boundary(
    dirs: &super::spec::SpecDirs<'_>,
    normalized_target: &Path,
    target_dir: &Path,
) -> Option<PathBuf> {
    if pnpm_fs::is_subdir(dirs.workspace_root, normalized_target) {
        return target_dir_within_workspace(dirs.canonical_paths, dirs.workspace_root, target_dir)
            .ok();
    }
    if std::fs::symlink_metadata(normalized_target)
        .is_ok_and(|metadata| metadata.file_type().is_symlink())
    {
        return None;
    }
    target_dir_within_workspace(dirs.canonical_paths, normalized_target, target_dir).ok()
}

fn publishing_project_manifest<'a>(
    dirs: &super::spec::SpecDirs<'a>,
    normalized_target: &Path,
) -> Option<Cow<'a, serde_json::Value>> {
    if !pnpm_fs::is_subdir(dirs.workspace_root, normalized_target) {
        return None;
    }
    if std::fs::symlink_metadata(normalized_target).is_ok()
        && target_dir_within_workspace(dirs.canonical_paths, dirs.workspace_root, normalized_target)
            .is_err()
    {
        return None;
    }
    for candidate in normalized_target.ancestors().skip(1) {
        if !pnpm_fs::is_subdir(dirs.workspace_root, candidate) {
            return None;
        }
        if let Some(manifest) = project_or_disk_manifest(dirs, candidate)
            && pnpm_package_manifest::publishes_from_directory(
                candidate,
                &manifest,
                normalized_target,
            )
        {
            return Some(manifest);
        }
    }
    None
}

/// The canonical workspace root, when `target_dir` lies inside it both as
/// written and with symlinks resolved. A loaded project is held to this too,
/// so a project directory that links outside the workspace is not trusted.
fn target_dir_within_workspace(
    canonical_paths: &pnpm_fs::CanonicalPaths,
    workspace_root: &Path,
    target_dir: &Path,
) -> Result<PathBuf, ()> {
    if !pnpm_fs::is_subdir(workspace_root, target_dir) {
        return Err(());
    }
    let canonical_root = canonical_paths
        .canonicalize(workspace_root)
        .map_err(|_| ())?;
    let canonical_target = canonical_paths
        .canonicalize(target_dir)
        .map_err(|_| ())?;
    if !pnpm_fs::is_subdir(&canonical_root, &canonical_target) {
        return Err(());
    }
    Ok(canonical_root)
}

/// Whether `key` is a `<name>@file://...` directory entry synthesized by
/// `pnpm deploy`, whose snapshot already carries the source workspace's
/// resolved catalog entries, overrides, extensions, hooks, and peer bindings.
///
/// Requires `dep_spec.version` to use the `<name>@file://...` alias shape and
/// distinguishes a two-slash `file://server/share/...` deploy URL from an
/// ordinary Windows UNC `file:` dependency whose `resolution.directory` is the
/// same `//server/share/...` string.
pub(in crate::install::lockfile_freshness) fn is_deploy_file_url_key(
    dep_spec: &pnpm_lockfile::ResolvedDependencySpec,
    key: &pnpm_lockfile::PackageKey,
    directory: &str,
) -> bool {
    matches!(dep_spec.version, pnpm_lockfile::ImporterDepVersion::Alias(_))
        && matches!(
            key.suffix.version(),
            pnpm_lockfile::VersionPart::File(path)
                if !pnpm_lockfile::is_local_tarball_path(path)
                    && (path.starts_with("///") || (path.starts_with("//") && path != directory)),
        )
}
