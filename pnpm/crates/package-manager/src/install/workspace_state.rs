use super::{
    BTreeMap, Catalogs, Clock, Config, DependencyGroup, HashSet, Host, IncludedDependencies,
    LazyLockfile, MaybeLazyLockfile, NodeLinker, OptimisticRepeatInstallCheck,
    OptimisticRepeatInstallDecision, PackageManifest, Path, PathBuf, ProjectEntry, ProjectMutation,
    WorkspaceState, check_optimistic_repeat_install, get_catalogs_from_workspace_manifest,
    gvs_build_marker_present, gvs_build_markers_may_require_recovery, load_workspace_projects,
    manifest_string_field, unapproved_recorded_ignored_builds,
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
    let UpToDateFastPathCheck {
        config,
        manifest,
        dependency_groups,
        node_linker,
        supported_architectures,
    } = check;
    let included = IncludedDependencies {
        dependencies: dependency_groups.contains(&DependencyGroup::Prod),
        dev_dependencies: dependency_groups.contains(&DependencyGroup::Dev),
        optional_dependencies: dependency_groups.contains(&DependencyGroup::Optional),
    };
    let manifest_dir = manifest.path().parent()?;
    let workspace_dir_opt = configured_or_discovered_workspace_dir(config, manifest_dir).ok()?;
    let workspace_root = workspace_dir_opt.clone().unwrap_or_else(|| manifest_dir.to_path_buf());
    let workspace_manifest = match workspace_dir_opt.as_deref() {
        Some(dir) => pnpm_workspace::read_workspace_manifest(dir).ok()?,
        None => None,
    };
    let catalogs = match config.catalogs.clone() {
        Some(catalogs) => catalogs,
        None => get_catalogs_from_workspace_manifest(workspace_manifest.as_ref()).ok()?,
    };
    let workspace_projects =
        load_workspace_projects(&workspace_root, workspace_manifest.as_ref()).ok()?;
    let project_manifests = build_project_manifests_list(manifest, workspace_projects.as_deref());
    // The lockfile the install wrote sits at its `lockfileDir`, which
    // both `sharedWorkspaceLockfile: false` and an explicit pin move away
    // from the discovered workspace root. The workspace *state* keeps its
    // own root, which only a pin moves — `state_root` below.
    let lockfile_root = lockfile_root_for(config, workspace_dir_opt.as_deref(), manifest_dir);
    let state_root = config.lockfile_dir.clone().unwrap_or_else(|| workspace_root.clone());
    let lockfile = if config.lockfile {
        LazyLockfile::deferred(lockfile_root.clone(), config.wanted_lockfile_selection())
    } else {
        LazyLockfile::disabled()
    };
    // Under `strictDepBuilds`, a recorded-and-still-unapproved ignored
    // build must keep the install failing — never let the pre-runtime
    // fast path report up-to-date and exit 0. Returning `None` falls
    // through to the full `Install::run`, whose optimistic branch raises
    // `ERR_PNPM_IGNORED_BUILDS`. A corrupt / unreadable `.modules.yaml`
    // is treated conservatively the same way (its `Err` can't prove the
    // absence of recorded ignored builds).
    if config.strict_dep_builds {
        match pnpm_modules_yaml::read_modules_layout::<Host>(&config.modules_dir) {
            Ok(Some(modules)) => match unapproved_recorded_ignored_builds(&modules, config) {
                Ok(Some(_)) => return None,
                Ok(None) => {}
                // Unreadable state or a malformed `allowBuilds`: force the
                // full install rather than reporting up-to-date.
                Err(_) => return None,
            },
            Ok(None) => {}
            Err(_) => return None,
        }
    }
    let up_to_date = check_optimistic_repeat_install(&OptimisticRepeatInstallCheck {
        workspace_root: &state_root,
        config,
        node_linker: *node_linker,
        included,
        supported_architectures: supported_architectures.as_ref(),
        project_manifests: &project_manifests,
        is_workspace_install: workspace_manifest.is_some(),
        lockfile: MaybeLazyLockfile::Lazy(&lockfile),
        catalogs: &catalogs,
    }) == OptimisticRepeatInstallDecision::UpToDate;
    if !up_to_date {
        return None;
    }
    if gvs_build_markers_may_require_recovery(config) {
        let wanted = lockfile.get().ok().flatten()?;
        let effective_node_version = super::effective_node_version(config, manifest);
        if gvs_build_marker_present(
            wanted,
            config,
            &lockfile_root,
            effective_node_version.as_deref(),
        ) {
            return None;
        }
    }
    Some(UpToDateWorkspace {
        root: state_root,
        project_count: workspace_projects.as_ref().map(Vec::len),
    })
}

/// Discovery twin of [`install_already_up_to_date`] for the
/// verify-deps-before-run gate: assemble the same
/// [`OptimisticRepeatInstallCheck`] inputs from a bare directory and run
/// [`crate::check_deps_status_before_run`].
///
/// Returns `None` when `dir` has no manifest and no enclosing workspace:
/// the run/exec command is about to fail with its own missing-manifest
/// error, and reporting "outdated" would only trigger an install that
/// can do nothing but crash with `NO_PKG_MANIFEST` (the same guard
/// pnpm's `checkDepsStatus` applies when it finds neither a root
/// manifest nor a workspace).
///
/// Any other discovery failure conservatively reports the dependencies
/// as unverifiable ("Cannot check whether dependencies are outdated"),
/// matching pnpm's catch-all: in the worst case the configured action
/// runs a redundant install.
#[must_use]
pub fn check_deps_status_before_run_at(
    dir: &Path,
    config: &Config,
) -> Option<crate::RunDepsStatus> {
    let cannot_check = || {
        Some(crate::RunDepsStatus::Outdated {
            issue: "Cannot check whether dependencies are outdated".to_string(),
            install_args: Vec::new(),
        })
    };
    let Ok(workspace_dir_opt) = configured_or_discovered_workspace_dir(config, dir) else {
        return cannot_check();
    };
    let workspace_root = workspace_dir_opt.clone().unwrap_or_else(|| dir.to_path_buf());
    let root_manifest = match pnpm_workspace::read_project_manifest_only(&workspace_root) {
        Ok(manifest) => manifest,
        Err(pnpm_workspace::ReadProjectManifestOnlyError::NoImporterManifestFound { .. })
            if workspace_dir_opt.is_none() =>
        {
            return None;
        }
        Err(_) => return cannot_check(),
    };
    let workspace_manifest = match workspace_dir_opt.as_deref() {
        Some(dir) => match pnpm_workspace::read_workspace_manifest(dir) {
            Ok(manifest) => manifest,
            Err(_) => return cannot_check(),
        },
        None => None,
    };
    // A pinned `lockfileDir` is where the install left the state and the
    // lockfile; the root manifest above stays where the workspace put it.
    let lockfile_root = config.lockfile_dir.clone().unwrap_or_else(|| workspace_root.clone());
    // pnpm reports "cannot check" straight from the missing workspace
    // state, before any project discovery — a fresh project (the common
    // out-of-sync case) must not pay for the workspace-projects walk
    // only to reach the same verdict inside the check.
    let Ok(Some(workspace_state)) = pnpm_workspace_state::load_workspace_state(&lockfile_root)
    else {
        return cannot_check();
    };
    let catalogs = match config.catalogs.clone() {
        Some(catalogs) => catalogs,
        None => match get_catalogs_from_workspace_manifest(workspace_manifest.as_ref()) {
            Ok(catalogs) => catalogs,
            Err(_) => return cannot_check(),
        },
    };
    let Ok(workspace_projects) =
        load_workspace_projects(&workspace_root, workspace_manifest.as_ref())
    else {
        return cannot_check();
    };
    let project_manifests =
        build_project_manifests_list(&root_manifest, workspace_projects.as_deref());
    let lockfile = if config.lockfile {
        LazyLockfile::deferred(lockfile_root.clone(), config.wanted_lockfile_selection())
    } else {
        LazyLockfile::disabled()
    };
    Some(crate::check_deps_status_before_run(
        &OptimisticRepeatInstallCheck {
            workspace_root: &lockfile_root,
            config,
            node_linker: config.node_linker,
            supported_architectures: config.supported_architectures.as_ref(),
            // The gate ignores dependency-group drift, so the groups only
            // shape the settings snapshot written back after a passing
            // content check — where the recorded values win anyway.
            included: IncludedDependencies {
                dependencies: true,
                dev_dependencies: true,
                optional_dependencies: true,
            },
            project_manifests: &project_manifests,
            is_workspace_install: workspace_manifest.is_some(),
            lockfile: MaybeLazyLockfile::Lazy(&lockfile),
            catalogs: &catalogs,
        },
        &workspace_state,
    ))
}

pub(super) fn build_project_manifests_list<'a>(
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

pub(super) fn build_root_importer_project_manifests_list<'a>(
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

pub(super) fn build_selected_project_manifests_list<'a>(
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
pub(super) struct ProjectDirMatcher {
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

pub(super) struct ProjectScriptsInputs<'a, 'manifest> {
    pub(super) mutation: ProjectMutation,
    pub(super) workspace_root: &'a Path,
    /// The project the command was run in, which is the sole mutated
    /// importer of an unfiltered `add` / `update`.
    pub(super) active_project_dir: &'a Path,
    /// The `--filter` / `-r` selection, when the run was narrowed to one.
    pub(super) selected_dirs: Option<&'a HashSet<PathBuf>>,
    /// Every project of the workspace.
    pub(super) project_manifests: &'a [(PathBuf, &'manifest PackageManifest)],
    /// The subset this run materialized. A project outside it has no
    /// linked `node_modules` for its scripts to import, so it never runs
    /// them even when the mutated set nominally covers it.
    pub(super) materialized_project_manifests: &'a [(PathBuf, &'manifest PackageManifest)],
}

/// The projects that fire their own lifecycle scripts at the end of this
/// run, following pnpm's mutated-importer rule (see [`ProjectMutation`]).
pub(super) fn projects_running_own_scripts<'manifest>(
    inputs: &ProjectScriptsInputs<'_, 'manifest>,
) -> Vec<(PathBuf, &'manifest PackageManifest)> {
    let ProjectScriptsInputs {
        mutation,
        workspace_root,
        active_project_dir,
        selected_dirs,
        project_manifests,
        materialized_project_manifests,
    } = *inputs;
    let full_install = match mutation {
        ProjectMutation::NoInstall | ProjectMutation::UninstallSome => return Vec::new(),
        ProjectMutation::InstallWorkspace => return materialized_project_manifests.to_vec(),
        ProjectMutation::InstallSelected => true,
        ProjectMutation::InstallSome => false,
    };
    let mutated_dirs = match selected_dirs {
        Some(selected_dirs) => {
            selected_dirs.iter().map(|dir| pnpm_fs::lexical_normalize(dir)).collect()
        }
        None => HashSet::from([pnpm_fs::lexical_normalize(active_project_dir)]),
    };
    // pnpm's recursive dispatch pushes the workspace root into the
    // mutated importers as a plain `mutation: 'install'` whenever the
    // selection leaves it out, so the root installs in full — and runs
    // its own scripts — even when the command was pointed elsewhere.
    let workspace_root = pnpm_fs::lexical_normalize(workspace_root);
    let root_was_pushed_in = !mutated_dirs.contains(&workspace_root);
    let is_pushed_root = |project_dir: &Path| root_was_pushed_in && project_dir == workspace_root;
    // A run that mutates only part of the workspace materializes the rest
    // from the lockfile alone; pnpm runs the scripts of everything it did
    // mutate, whatever the mutation. Only when the mutated set covers the
    // whole workspace does the `mutation === 'install'` filter decide.
    let covers_workspace = project_manifests.iter().all(|(project_dir, _)| {
        let project_dir = pnpm_fs::lexical_normalize(project_dir);
        mutated_dirs.contains(&project_dir) || is_pushed_root(&project_dir)
    });
    materialized_project_manifests
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

pub(super) fn selected_manifest_freshness_inputs<'a>(
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

pub(crate) fn configured_or_discovered_workspace_dir(
    config: &Config,
    manifest_dir: &Path,
) -> Result<Option<PathBuf>, pnpm_workspace::FindWorkspaceDirError> {
    match config.workspace_dir.clone() {
        Some(workspace_dir) => Ok(Some(workspace_dir)),
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
