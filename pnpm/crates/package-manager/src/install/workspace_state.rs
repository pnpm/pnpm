pub(super) use projects::{
    ProjectScriptsInputs, build_root_importer_project_manifests_list,
    build_selected_project_manifests_list, mutated_project_dirs, projects_running_own_scripts,
    selected_manifest_freshness_inputs,
};
pub(crate) use projects::{build_project_manifests_list, replace_project_manifest};
pub(crate) use write::{CarriedSettings, StateProjects, StateWrite, recorded_workspace_state};

mod projects;

mod write;

use super::{
    BTreeMap, Catalogs, Clock, Config, IncludedDependencies, NodeLinker, PackageManifest, Path,
    PathBuf, ProjectEntry, WorkspaceState, manifest_string_field,
};
use crate::install_state::last_validated_timestamp;
use pnpm_reporter::Reporter;
use pnpm_workspace_state::update_workspace_state_in_modules;

/// Persist `.pnpm-workspace-state-v1.json`, warning instead of failing
/// the command when the write is lost.
///
/// The state file is a cache: the next command's content check
/// re-derives everything in it, so a lost write must not fail an
/// install whose `node_modules` and lockfile are already committed
/// ([#14550](https://github.com/pnpm/pnpm/issues/14550)).
pub(crate) fn update_workspace_state_or_warn<Reporter: self::Reporter>(
    workspace_root: &Path,
    state: &WorkspaceState,
    config: &Config,
) {
    let modules_dir = config.workspace_state_modules_dir(workspace_root);
    if let Err(error) = update_workspace_state_in_modules(&modules_dir, state) {
        tracing::warn!(
            target: "pacquet::install",
            ?error,
            "Failed to write the workspace state",
        );
        pnpm_reporter::emit_global_warning::<Reporter>(&format!(
            "Failed to write the workspace state: {error}",
        ));
    }
}

/// The workspace root the run belongs to: the configured one, else the
/// nearest ancestor carrying a `pnpm-workspace.yaml`.
///
/// Yields `None` under [`Config::workspace_search_skipped`], where the
/// ancestor walk would re-adopt the very `pnpm-workspace.yaml` that
/// `--ignore-workspace` asked to ignore. Only that walk is suppressed:
/// a caller that pinned [`Config::workspace_dir`] keeps it, which is how a
/// global install anchors itself under the global packages dir.
pub(crate) fn configured_or_discovered_workspace_dir(
    config: &Config,
    manifest_dir: &Path,
) -> Result<Option<PathBuf>, pnpm_workspace::FindWorkspaceDirError> {
    match &config.workspace_dir {
        Some(workspace_dir) => Ok(Some(workspace_dir.clone())),
        None if config.workspace_search_skipped => Ok(None),
        None => pnpm_workspace::find_workspace_dir(manifest_dir),
    }
}

/// The directory `pnpm-lock.yaml` lives in, which is what importer ids,
/// reporter prefixes and the workspace-state file are all named relative
/// to. A pinned `lockfileDir` wins outright; otherwise dedicated
/// per-project lockfiles (`sharedWorkspaceLockfile: false`) anchor it at
/// the active project rather than the workspace root, mirroring pnpm's
/// `lockfileDir ?? (sharedWorkspaceLockfile ? workspaceDir : projectDir)`.
///
/// Unlike [`Config::lockfile_dir_for`], this also *discovers* the
/// workspace root when the config carries none.
///
/// Every caller that names something by importer id has to derive it the
/// same way the install does, or the two disagree about which importer an
/// entry belongs to.
pub(crate) fn lockfile_root_dir(
    config: &Config,
    manifest_dir: &Path,
) -> Result<PathBuf, pnpm_workspace::FindWorkspaceDirError> {
    if let Some(lockfile_dir) = &config.lockfile_dir {
        return Ok(lockfile_dir.clone());
    }
    if !config.shared_workspace_lockfile {
        return Ok(manifest_dir.to_path_buf());
    }
    Ok(configured_or_discovered_workspace_dir(config, manifest_dir)?
        .unwrap_or_else(|| manifest_dir.to_path_buf()))
}

/// [`lockfile_root_dir`] for a caller that has already resolved the
/// workspace dir, so the ancestor walk is not repeated.
pub(crate) fn lockfile_root_for(
    config: &Config,
    workspace_dir: Option<&Path>,
    manifest_dir: &Path,
) -> PathBuf {
    if let Some(lockfile_dir) = &config.lockfile_dir {
        return lockfile_dir.clone();
    }
    if !config.shared_workspace_lockfile {
        return manifest_dir.to_path_buf();
    }
    workspace_dir.map_or_else(|| manifest_dir.to_path_buf(), Path::to_path_buf)
}

/// Build the `name → version → WorkspacePackage` lookup the npm
/// resolver consults for `workspace:` specs. Returns `None` when
/// `projects` is `None` (no workspace) so any `workspace:` spec the
/// manifest happens to carry surfaces
/// [`pnpm_resolving_npm_resolver::ResolveFromWorkspaceError::WorkspacePackagesNotLoaded`].
///
/// The map is a name/version index of per-project `WorkspacePackage`
/// entries (`{ rootDir, manifest }`) consumed by the resolver.
/// Projects whose manifest lacks a name are skipped. A missing or null
/// version is indexed as `0.0.0`; malformed non-string versions are skipped.
#[must_use]
pub fn build_workspace_packages_map(
    projects: Option<&[pnpm_workspace::Project]>,
) -> Option<pnpm_resolving_resolver_base::WorkspacePackages> {
    let projects = projects?;
    let mut map: pnpm_resolving_resolver_base::WorkspacePackages =
        std::collections::BTreeMap::new();
    for project in projects {
        insert_workspace_package(
            &mut map,
            &project.root_dir,
            &project.manifest,
            project.dependency_manifest.as_ref().unwrap_or(&project.manifest),
        );
    }
    Some(map)
}

/// [`build_workspace_packages_map`] over `(root_dir, manifest)` pairs, with
/// the projects' dependency manifests (see
/// [`super::dependency_manifests_by_dir`]) in place of their importer
/// manifests where they have one.
pub(crate) fn build_workspace_packages_map_from_manifests(
    projects: &[(PathBuf, &PackageManifest)],
    dependency_manifests: Option<&super::ProjectManifestsByDir<'_>>,
) -> pnpm_resolving_resolver_base::WorkspacePackages {
    let mut map = std::collections::BTreeMap::new();
    for (root_dir, manifest) in projects {
        let dependency_manifest = dependency_manifests
            .and_then(|by_dir| {
                by_dir
                    .get(&pnpm_fs::lexical_normalize(root_dir))
                    .copied()
            })
            .unwrap_or(manifest);
        insert_workspace_package(&mut map, root_dir, manifest, dependency_manifest);
    }
    map
}

pub(crate) fn workspace_packages_for_freshness(
    config: &Config,
    is_workspace_install: bool,
    projects: &[(PathBuf, &PackageManifest)],
    dependency_manifests: Option<&super::ProjectManifestsByDir<'_>>,
) -> Option<pnpm_resolving_resolver_base::WorkspacePackages> {
    (is_workspace_install
        && config.exclude_links_from_lockfile
        && config.link_workspace_packages.enabled_at_depth(0))
    .then(|| build_workspace_packages_map_from_manifests(projects, dependency_manifests))
}

fn insert_workspace_package(
    map: &mut pnpm_resolving_resolver_base::WorkspacePackages,
    root_dir: &Path,
    identity_manifest: &PackageManifest,
    dependency_manifest: &PackageManifest,
) {
    let Some(name) = manifest_string_field(identity_manifest, "name") else { return };
    let version = match identity_manifest.value().get("version") {
        None => "0.0.0".to_string(),
        Some(value) if value.is_null() => "0.0.0".to_string(),
        Some(value) => {
            let Some(version) = value.as_str() else { return };
            version.to_string()
        }
    };
    map.entry(name)
        .or_default()
        .insert(
            version,
            pnpm_resolving_resolver_base::WorkspacePackage {
                root_dir: root_dir.to_path_buf(),
                manifest: dependency_manifest.value().clone(),
            },
        );
}

/// Build the `projects` map for [`WorkspaceState`] from the
/// in-memory `(root_dir, manifest)` list the caller already
/// assembled.
///
/// Pure in-memory: no file I/O, no read-failure warnings, no lockfile
/// or importer traversal. Every project — root and siblings alike —
/// reuses the [`PackageManifest`] reference already loaded for the
/// install dispatch.
pub(super) fn build_projects_map(
    project_manifests: &[(std::path::PathBuf, &PackageManifest)],
) -> BTreeMap<String, ProjectEntry> {
    project_manifests
        .iter()
        .map(|(project_dir, manifest)| {
            let entry = ProjectEntry {
                name: manifest_string_field(manifest, "name"),
                version: manifest_string_field(manifest, "version"),
                has_modules_dir: false,
            };
            (project_dir.to_string_lossy().into_owned(), entry)
        })
        .collect()
}

/// Assemble the [`WorkspaceState`] payload for
/// [`pnpm_workspace_state::update_workspace_state`], for
/// [`crate::install_state::validation::record_validation`] to
/// complete once the caller has settled its settings.
///
/// Records the projects pacquet just materialized plus the resolved
/// settings the install used, and the [`last_validated_timestamp`].
/// Settings pacquet does not track yet (e.g. `peersSuffixMaxLength`)
/// are omitted; pnpm's `checkDepsStatus`
/// only iterates fields present in the serialized object, so an
/// absent key is silently skipped rather than treated as a drift.
#[expect(
    clippy::too_many_arguments,
    reason = "the workspace-state writer records the install run's resolved inputs"
)]
pub(crate) fn build_workspace_state<Sys: Clock>(
    workspace_root: &Path,
    config: &Config,
    node_linker: NodeLinker,
    included: IncludedDependencies,
    supported_architectures: Option<&pnpm_package_is_installable::SupportedArchitectures>,
    catalogs: &Catalogs,
    project_manifests: crate::DeclaredManifests<'_>,
    filesystem_now_ms: Option<i64>,
) -> WorkspaceState {
    let project_manifests = project_manifests.as_slice();
    let mut state = WorkspaceState {
        last_validated_timestamp: last_validated_timestamp::<Sys>(
            workspace_root,
            config,
            project_manifests,
            filesystem_now_ms,
        ),
        projects: build_projects_map(project_manifests),
        pnpmfiles: crate::install_state::current_pnpmfiles(workspace_root, config),
        filtered_install: false,
        config_dependencies: config.config_dependencies.clone(),
        validation: None,
        // Settings construction is shared with
        // `install_state::current_settings` so the
        // freshness check sees the same byte shape this writer
        // produces. Keeping the construction in one place guarantees
        // adding a field on one side doesn't silently flip the other
        // into "drift" on the next install.
        settings: crate::install_state::current_settings_with_catalogs(
            config,
            node_linker,
            included,
            supported_architectures,
            catalogs,
        ),
    };
    // Frozen installs share this builder and cannot establish a deduplication
    // baseline, nor vouch for the lockfile's registry keys, nor say which
    // routes its plain keys resolved through.
    state.settings.auto_dedupe = None;
    state.settings.registry_prefixes = None;
    state.settings.default_registry = None;
    state.settings.registry_routes = None;
    if node_linker == NodeLinker::Hoisted {
        record_hoisted_modules_dirs(&mut state.projects, project_manifests);
    }
    state
}

/// Whether every `packages:` key of `lockfile` is the one the declared
/// registry prefixes call for, so the state may record them. A missing
/// lockfile verifies nothing.
pub(crate) fn registry_keys_current(
    config: &Config,
    lockfile: Option<&pnpm_lockfile::Lockfile>,
) -> bool {
    lockfile.is_some_and(|lockfile| {
        pnpm_lockfile::check_lockfile_registry_keys(lockfile, &config.registry_context()).is_ok()
    })
}

/// The `autoDedupe` an install records in the workspace state.
///
/// With `lockfile.includeResolutionSettings`, the lockfile records
/// `autoDedupe` and its freshness check holds that value to the config, so
/// every install that kept the lockfile records the config's value.
/// Otherwise only a resolving install establishes the dedupe baseline, and
/// `established` is what this write knows of it.
pub(crate) fn recorded_auto_dedupe(config: &Config, established: Option<bool>) -> Option<bool> {
    if config.lockfile && config.lockfile_include_resolution_settings {
        return config.auto_dedupe.then_some(true);
    }
    established
}

/// Set [`ProjectEntry::has_modules_dir`] for each project the hoisted
/// install left with its own modules directory.
fn record_hoisted_modules_dirs(
    projects: &mut BTreeMap<String, ProjectEntry>,
    project_manifests: &[(PathBuf, &PackageManifest)],
) {
    for (root_dir, _) in project_manifests {
        if let Some(entry) = projects.get_mut(&*root_dir.to_string_lossy()) {
            entry.has_modules_dir =
                crate::install_state::hoisted_project_modules_dir(root_dir).is_dir();
        }
    }
}
