pub use discovery::check_deps_status_before_run_at;
pub(super) use projects::{
    ProjectScriptsInputs, build_project_manifests_list, build_root_importer_project_manifests_list,
    build_selected_project_manifests_list, projects_running_own_scripts,
    selected_manifest_freshness_inputs,
};

mod projects;

mod discovery;

use super::{
    BTreeMap, Catalogs, Clock, Config, DependencyGroup, Host, IncludedDependencies, LazyLockfile,
    MaybeLazyLockfile, NodeLinker, OptimisticRepeatInstallCheck, OptimisticRepeatInstallDecision,
    PackageManifest, Path, PathBuf, ProjectEntry, WorkspaceState, check_optimistic_repeat_install,
    get_catalogs_from_workspace_manifest, gvs_build_marker_present,
    gvs_build_markers_may_require_recovery, load_workspace_projects, manifest_string_field,
    unapproved_recorded_ignored_builds,
};
use crate::optimistic_repeat_install::{refreshed_validation_baseline_ms, validation_baseline_ms};

/// Inputs for [`install_already_up_to_date`].
pub struct UpToDateFastPathCheck<'a> {
    pub config: &'a Config,
    pub manifest: &'a PackageManifest,
    pub dependency_groups: Vec<DependencyGroup>,
    pub node_linker: NodeLinker,
    /// The CLI-merged effective `supportedArchitectures` (yaml plus
    /// `--cpu` / `--os` / `--libc`) — the fast path must not report
    /// "Already up to date" when a flag changed the target platforms.
    pub supported_architectures: Option<pnpm_package_is_installable::SupportedArchitectures>,
}

/// Pre-runtime twin of the repeat-install short-circuit inside
/// [`super::Install::run`]: same workspace discovery, same
/// What an up-to-date install reports about the workspace it covered.
#[derive(Debug, PartialEq, Eq)]
pub struct UpToDateWorkspace {
    /// The workspace root — the reporter `prefix` for the "Already up to
    /// date" emission.
    pub root: PathBuf,
    /// How many projects the workspace has, or `None` when there is no
    /// `pnpm-workspace.yaml`. Feeds the `pnpm:scope` report, which the
    /// full install path derives from the same walk.
    pub project_count: Option<usize>,
}

/// [`check_optimistic_repeat_install`] inputs, callable from a
/// synchronous context so the CLI can finish an up-to-date install
/// before paying for the async runtime, the HTTP client, and the
/// state setup. Returns what the short-circuit covered when the install
/// can take it.
///
/// Failures deliberately collapse to `None`: the caller falls through
/// to the full install path, which reproduces the failure with its
/// established error shape.
#[must_use]
pub fn install_already_up_to_date(check: &UpToDateFastPathCheck<'_>) -> Option<UpToDateWorkspace> {
    let manifest_dir = check.manifest.path().parent()?;
    let workspace_dir_opt =
        configured_or_discovered_workspace_dir(check.config, manifest_dir).ok()?;
    let workspace_root = workspace_dir_opt.clone().unwrap_or_else(|| manifest_dir.to_path_buf());
    let (workspace_manifest, catalogs) =
        fast_path_workspace_context(check.config, workspace_dir_opt.as_deref())?;
    let workspace_projects =
        load_workspace_projects(&workspace_root, workspace_manifest.as_ref()).ok()?;
    let project_manifests =
        build_project_manifests_list(check.manifest, workspace_projects.as_deref());
    // The lockfile the install wrote sits at its `lockfileDir`, which
    // both `sharedWorkspaceLockfile: false` and an explicit pin move away
    // from the discovered workspace root. The workspace *state* keeps its
    // own root, which only a pin moves — `state_root` below.
    let lockfile_root = lockfile_root_for(check.config, workspace_dir_opt.as_deref(), manifest_dir);
    let state_root = check.config.lockfile_dir.clone().unwrap_or_else(|| workspace_root.clone());
    let lockfile = lazy_wanted_lockfile(check.config, &lockfile_root);
    if strict_dep_builds_blocks_fast_path(check.config) {
        return None;
    }
    if check_optimistic_repeat_install(&OptimisticRepeatInstallCheck {
        workspace_root: &state_root,
        config: check.config,
        node_linker: check.node_linker,
        included: super::included_dependencies(&check.dependency_groups),
        supported_architectures: check.supported_architectures.as_ref(),
        project_manifests: &project_manifests,
        is_workspace_install: workspace_manifest.is_some(),
        lockfile: MaybeLazyLockfile::Lazy(&lockfile),
        catalogs: &catalogs,
    }) != OptimisticRepeatInstallDecision::UpToDate
    {
        return None;
    }
    ensure_gvs_builds_complete(check, &lockfile, &lockfile_root)?;
    Some(UpToDateWorkspace {
        root: state_root,
        project_count: workspace_projects.as_ref().map(Vec::len),
    })
}

fn fast_path_workspace_context(
    config: &Config,
    workspace_dir: Option<&Path>,
) -> Option<(Option<pnpm_workspace::WorkspaceManifest>, super::Catalogs)> {
    let workspace_manifest =
        workspace_dir.map(pnpm_workspace::read_workspace_manifest).transpose().ok()?.flatten();
    let catalogs = match config.catalogs.clone() {
        Some(catalogs) => catalogs,
        None => get_catalogs_from_workspace_manifest(workspace_manifest.as_ref()).ok()?,
    };
    Some((workspace_manifest, catalogs))
}

fn ensure_gvs_builds_complete(
    check: &UpToDateFastPathCheck<'_>,
    lockfile: &pnpm_lockfile::LazyLockfile,
    lockfile_root: &Path,
) -> Option<()> {
    if gvs_build_markers_may_require_recovery(check.config)
        && gvs_build_marker_present(
            lockfile.get().ok().flatten()?,
            check.config,
            lockfile_root,
            super::effective_node_version(check.config, check.manifest).as_deref(),
        )
    {
        return None;
    }
    Some(())
}

pub(crate) fn configured_or_discovered_workspace_dir(
    config: &Config,
    manifest_dir: &Path,
) -> Result<Option<PathBuf>, pnpm_workspace::FindWorkspaceDirError> {
    match config.workspace_dir.clone() {
        Some(workspace_dir) => Ok(Some(workspace_dir)),
        // `--ignore-workspace` means "run as if this project were
        // standalone", so a `None` here is not "not resolved yet": the
        // config layer deliberately refused to resolve one, and the
        // ancestor walk would re-adopt the very `pnpm-workspace.yaml` the
        // user asked to ignore. Only the fallback is suppressed — a
        // caller that pinned `workspace_dir` keeps it, which is how a
        // global install anchors itself under the global packages dir.
        //
        // `workspace_search_skipped` rather than `ignore_workspace`
        // records this, because only the CLI flag suppresses discovery: a
        // value from `pnpm-workspace.yaml` or
        // `PNPM_CONFIG_IGNORE_WORKSPACE` reaches the merged boolean too
        // late to affect it, and must not turn the project standalone.
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
    if let Some(lockfile_dir) = config.lockfile_dir.clone() {
        return Ok(lockfile_dir);
    }
    if !config.shared_workspace_lockfile {
        return Ok(manifest_dir.to_path_buf());
    }
    Ok(configured_or_discovered_workspace_dir(config, manifest_dir)?
        .unwrap_or_else(|| manifest_dir.to_path_buf()))
}

/// [`lockfile_root_dir`] for a caller that has already resolved the
/// workspace dir, so the ancestor walk is not repeated.
pub(super) fn lockfile_root_for(
    config: &Config,
    workspace_dir: Option<&Path>,
    manifest_dir: &Path,
) -> PathBuf {
    if let Some(lockfile_dir) = config.lockfile_dir.clone() {
        return lockfile_dir;
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
/// Index the workspace projects by package name and version — the
/// resolver's view of what the workspace publishes, and the link
/// targets `pacquet update --workspace` re-points dependencies at. A
/// project without a name publishes nothing; a missing or null version
/// reads as `0.0.0`, matching pnpm.
#[must_use]
pub fn build_workspace_packages_map(
    projects: Option<&[pnpm_workspace::Project]>,
) -> Option<pnpm_resolving_resolver_base::WorkspacePackages> {
    let projects = projects?;
    let mut map: pnpm_resolving_resolver_base::WorkspacePackages =
        std::collections::BTreeMap::new();
    for project in projects {
        let Some(name) = manifest_string_field(&project.manifest, "name") else { continue };
        let version = match project.manifest.value().get("version") {
            None => "0.0.0".to_string(),
            Some(value) if value.is_null() => "0.0.0".to_string(),
            Some(value) => {
                let Some(version) = value.as_str() else { continue };
                version.to_string()
            }
        };
        map.entry(name).or_default().insert(
            version,
            pnpm_resolving_resolver_base::WorkspacePackage {
                root_dir: project.root_dir.clone(),
                // The map feeds workspace picks resolved as *dependencies*
                // (injected instances), so a project that splits its two
                // views contributes its dependency manifest here — see
                // `pnpm_workspace::Project::dependency_manifest`.
                manifest: project
                    .dependency_manifest
                    .as_ref()
                    .unwrap_or(&project.manifest)
                    .value()
                    .clone(),
            },
        );
    }
    Some(map)
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
            };
            (project_dir.to_string_lossy().into_owned(), entry)
        })
        .collect()
}

/// Assemble the [`WorkspaceState`] payload for
/// [`pnpm_workspace_state::update_workspace_state`].
///
/// Records the projects pacquet just materialized plus the resolved
/// settings the install used. `lastValidatedTimestamp` is the
/// [`validation_baseline_ms`] of the lockfile and manifests raised to
/// `filesystem_now_ms`, per [`refreshed_validation_baseline_ms`]; the
/// wall clock stands in only when nothing can be stat'd.
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
    project_manifests: &[(std::path::PathBuf, &PackageManifest)],
    filtered_install: bool,
    filesystem_now_ms: Option<i64>,
) -> WorkspaceState {
    WorkspaceState {
        last_validated_timestamp: refreshed_validation_baseline_ms(
            validation_baseline_ms(workspace_root, config, project_manifests)
                .unwrap_or_else(|| pnpm_workspace_state::millis_since_epoch(Sys::now())),
            filesystem_now_ms,
        ),
        projects: build_projects_map(project_manifests),
        pnpmfiles: crate::optimistic_repeat_install::current_pnpmfiles(workspace_root, config),
        filtered_install,
        config_dependencies: config.config_dependencies.clone(),
        // Settings construction is shared with
        // `optimistic_repeat_install::current_settings` so the
        // freshness check sees the same byte shape this writer
        // produces. Keeping the construction in one place guarantees
        // adding a field on one side doesn't silently flip the other
        // into "drift" on the next install.
        settings: crate::optimistic_repeat_install::current_settings_with_catalogs(
            config,
            node_linker,
            included,
            supported_architectures,
            catalogs,
        ),
    }
}

/// The wanted lockfile, read on first use — or a stand-in that never reads
/// one when the install is configured without a lockfile.
fn lazy_wanted_lockfile(config: &Config, lockfile_root: &Path) -> LazyLockfile {
    if config.lockfile {
        LazyLockfile::deferred(lockfile_root.to_path_buf(), config.wanted_lockfile_selection())
    } else {
        LazyLockfile::disabled()
    }
}

/// Under `strictDepBuilds`, a recorded-and-still-unapproved ignored build must
/// keep the install failing — never let the pre-runtime fast path report
/// up-to-date and exit 0. `true` falls through to the full `Install::run`,
/// whose optimistic branch raises `ERR_PNPM_IGNORED_BUILDS`. A corrupt or
/// unreadable `.modules.yaml` is treated conservatively the same way: its
/// `Err` cannot prove the absence of recorded ignored builds.
fn strict_dep_builds_blocks_fast_path(config: &Config) -> bool {
    if !config.strict_dep_builds {
        return false;
    }
    match pnpm_modules_yaml::read_modules_layout::<Host>(&config.modules_dir) {
        Ok(Some(modules)) => match unapproved_recorded_ignored_builds(&modules, config) {
            Ok(Some(_)) | Err(_) => true,
            Ok(None) => false,
        },
        Ok(None) => false,
        Err(_) => true,
    }
}
