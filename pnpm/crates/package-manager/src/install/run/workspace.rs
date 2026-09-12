use super::{
    super::{
        HashSet, InstallError, InstallRunOptions, LogEvent, LogLevel, PackageManifest, Path,
        PathBuf, Reporter, ScopeLog, build_project_manifests_list,
        build_root_importer_project_manifests_list, build_selected_project_manifests_list,
        configured_or_discovered_workspace_dir, get_catalogs_from_workspace_manifest,
        load_workspace_projects, lockfile_root_dir,
    },
    InstallOwned, InstallView, RunMode, UpToDateCheck, install_is_already_up_to_date,
};
use pnpm_config::Config;

/// The directories the run anchors on.
pub(super) struct WorkspaceDirs<'a> {
    manifest_dir: &'a Path,
    workspace_dir: Option<PathBuf>,
    workspace_manifest_dir: PathBuf,
    workspace_root: PathBuf,
}
impl<'a> WorkspaceDirs<'a> {
    fn read_manifest(&self) -> Result<Option<pnpm_workspace::WorkspaceManifest>, InstallError> {
        let read =
            self.workspace_dir.as_deref().map(pnpm_workspace::read_workspace_manifest).transpose();
        read.map_err(InstallError::ReadWorkspaceManifest).map(Option::flatten)
    }

    // Project root for the [bunyan]-envelope `prefix`. This is
    // emitted as `lockfileDir`, the directory containing
    // `pnpm-lock.yaml`. With workspace support that equals the
    // workspace root — pacquet finds it via [`find_workspace_dir`].
    // Falls back to the manifest's parent dir when no
    // `pnpm-workspace.yaml` exists in any ancestor (the
    // single-project case). Closes pnpm/pacquet#357.
    //
    // [bunyan]: <https://github.com/trentm/node-bunyan>
    fn find(install: InstallView<'a>) -> Result<Self, InstallError> {
        let manifest_dir =
            install.manifest.path().parent().expect("manifest path always has a parent dir");
        let workspace_dir = configured_or_discovered_workspace_dir(install.config, manifest_dir)
            .map_err(InstallError::FindWorkspaceDir)?;
        Ok(Self {
            manifest_dir,
            workspace_manifest_dir: workspace_dir
                .clone()
                .unwrap_or_else(|| manifest_dir.to_path_buf()),
            // Catalogs and workspace packages still come from the real
            // workspace dir, which `lockfile_root_dir` parts ways with
            // under `sharedWorkspaceLockfile: false`.
            workspace_root: lockfile_root_dir(install.config, manifest_dir)
                .map_err(InstallError::FindWorkspaceDir)?,
            workspace_dir,
        })
    }
}
/// The workspace the run installs into: the directories it anchors on,
/// its catalogs and its projects.
pub(super) struct InstallWorkspace<'a> {
    pub(super) manifest_dir: &'a Path,
    pub(super) workspace_dir: Option<PathBuf>,
    pub(super) workspace_manifest_dir: PathBuf,
    pub(super) workspace_root: PathBuf,
    workspace_manifest: Option<pnpm_workspace::WorkspaceManifest>,
    pub(super) catalog_context_present: bool,
    pub(super) catalogs: super::super::Catalogs,
    pub(super) prefix: String,
    pub(super) workspace_projects_are_overridden: bool,
    pub(super) loaded_workspace_projects: Option<Vec<pnpm_workspace::Project>>,
}
/// The projects the run installs, and how a selection narrows them.
pub(super) struct InstallScope<'w> {
    pub(super) project_manifests: Vec<(PathBuf, &'w PackageManifest)>,
    pub(super) importers: ImporterSelection,
    pub(super) prune_stale_importers: bool,
}
/// The importers a selection narrows the run to.
pub(super) struct ImporterSelection {
    pub(super) real_importer_ids: HashSet<String>,
    pub(super) filtered_install: bool,
    pub(super) requested_importer_ids: Option<HashSet<String>>,
}
impl ImporterSelection {
    fn select(
        selection: Option<&crate::WorkspaceInstallSelection<'_>>,
        workspace_root: &Path,
        project_manifests: &[(PathBuf, &PackageManifest)],
    ) -> Self {
        let real_importer_ids = importer_ids(
            workspace_root,
            project_manifests.iter().map(|(project_dir, _)| project_dir.as_path()),
        );
        let filtered_install = selection.is_some_and(|selection| {
            importer_ids(workspace_root, selection.selected_dirs.iter().map(PathBuf::as_path))
                != real_importer_ids
        });
        Self {
            requested_importer_ids: filtered_install.then_some(selection).flatten().map(
                |selection| {
                    importer_ids(
                        workspace_root,
                        selection.install_dirs.iter().map(PathBuf::as_path),
                    )
                },
            ),
            real_importer_ids,
            filtered_install,
        }
    }
}
pub(super) fn importer_ids<'d>(
    workspace_root: &Path,
    dirs: impl Iterator<Item = &'d Path>,
) -> HashSet<String> {
    dirs.map(|project_dir| pnpm_workspace::importer_id_from_root_dir(workspace_root, project_dir))
        .collect()
}
impl<'a> InstallWorkspace<'a> {
    /// Consumes the catalogs and workspace-projects overrides off `owned`.
    pub(super) fn discover<Reporter: self::Reporter>(
        install: InstallView<'a>,
        owned: &mut InstallOwned,
        options: &InstallRunOptions<'_, '_>,
    ) -> Result<Self, InstallError> {
        let dirs = WorkspaceDirs::find(install)?;
        let workspace_manifest = dirs.read_manifest()?;
        let catalog_context_present = catalog_context_present(
            install.config,
            owned.catalogs_override.as_ref(),
            dirs.workspace_dir.as_ref(),
        );
        let catalogs = resolve_install_catalogs(
            install.config,
            owned.catalogs_override.take(),
            workspace_manifest.as_ref(),
        )?;
        // Walk every workspace project's `package.json` once. The
        // resulting `Vec` feeds both the up-to-date short-circuit
        // below and the fresh-install path's `workspace:`-spec lookup
        // / per-importer manifest list further down. `None` when no
        // `pnpm-workspace.yaml` exists in or above `workspace_root` —
        // single-project installs only have the root manifest, which
        // the short-circuit and the install paths both reach via
        // `manifest` directly.
        //
        // An embedder that supplies its importers in memory
        // (`workspace_projects_override`) bypasses the on-disk walk
        // entirely; the override's `Vec` is used verbatim.
        let workspace_projects_are_overridden = owned.workspace_projects_override.is_some();
        let loaded_workspace_projects = discovered_workspace_projects(
            options.selection.is_some(),
            owned.workspace_projects_override.take(),
            dirs.workspace_dir.as_deref().unwrap_or(&dirs.workspace_root),
            workspace_manifest.as_ref(),
        )?;
        report_discovered_scope::<Reporter>(
            install,
            options,
            &dirs,
            loaded_workspace_projects.as_deref(),
        );
        Ok(Self {
            // Use `to_string_lossy` rather than `to_str().expect(...)` so a
            // valid filesystem path with non-UTF-8 bytes (possible on Unix)
            // doesn't panic the installer. `prefix` is used only for
            // reporter envelopes, so a lossy conversion is acceptable —
            // the rest of the install path uses the same pattern for
            // paths threaded into log events.
            prefix: dirs.workspace_root.to_string_lossy().into_owned(),
            manifest_dir: dirs.manifest_dir,
            workspace_dir: dirs.workspace_dir,
            workspace_manifest_dir: dirs.workspace_manifest_dir,
            workspace_root: dirs.workspace_root,
            workspace_manifest,
            catalog_context_present,
            catalogs,
            workspace_projects_are_overridden,
            loaded_workspace_projects,
        })
    }
}
// In-memory mutation catalogs take precedence over hooked configuration and the workspace file.
// Filtered and dedicated-lockfile installs have already reported their own scope.
pub(super) fn report_discovered_scope<Reporter: self::Reporter>(
    install: InstallView<'_>,
    options: &InstallRunOptions<'_, '_>,
    dirs: &WorkspaceDirs,
    loaded_workspace_projects: Option<&[pnpm_workspace::Project]>,
) {
    let workspace_projects = options
        .selection
        .as_ref()
        .map_or_else(|| loaded_workspace_projects, |selection| Some(selection.all_projects));
    if options.selection.is_none() {
        emit_scope_log::<Reporter>(
            install.config,
            install.mutation,
            workspace_projects,
            dirs.workspace_dir.as_deref(),
        );
    }
}
pub(super) fn resolve_install_catalogs(
    config: &Config,
    catalogs_override: Option<super::super::Catalogs>,
    workspace_manifest: Option<&pnpm_workspace::WorkspaceManifest>,
) -> Result<super::super::Catalogs, InstallError> {
    Ok(match catalogs_override.or_else(|| config.catalogs.clone()) {
        Some(catalogs) => catalogs,
        None => get_catalogs_from_workspace_manifest(workspace_manifest)
            .map_err(InstallError::InvalidCatalogsConfiguration)?,
    })
}
/// The projects the run sees: the selection's when one narrows the run,
/// else what the workspace walk loaded.
pub(super) fn workspace_projects<'s>(
    loaded: Option<&'s [pnpm_workspace::Project]>,
    selection: Option<&crate::WorkspaceInstallSelection<'s>>,
) -> Option<&'s [pnpm_workspace::Project]> {
    selection.map_or(loaded, |selection| Some(selection.all_projects))
}
impl<'w> InstallScope<'w> {
    pub(super) fn select(
        install: InstallView<'w>,
        workspace_root: &Path,
        workspace_projects: Option<&'w [pnpm_workspace::Project]>,
        workspace_projects_are_overridden: bool,
        options: &InstallRunOptions<'w, 'w>,
    ) -> Self {
        let project_manifests = install_project_manifests(&ProjectManifestScope {
            manifest: install.manifest,
            selection: options.selection.as_ref(),
            workspace_root,
            workspace_projects,
            root_manifest_as_workspace_root: options.root_manifest_as_workspace_root,
            workspace_projects_are_overridden,
            config: install.config,
        });
        let importers = ImporterSelection::select(
            options.selection.as_ref(),
            workspace_root,
            &project_manifests,
        );
        // Only an install that covers a whole workspace sees the complete
        // project list, so only it may conclude that an importer the
        // lockfile records belongs to a project that is gone. This is
        // pnpm's `pruneLockfileImporters`, which its recursive install
        // defaults to the same condition (`pkgs.length ===
        // allProjects.length`) — outside a workspace there is no project
        // list to compare against.
        // A `NodeApiProject[]` handed in by an API consumer carries no
        // promise of listing every workspace project, so it cannot stand
        // in for the project list either.
        let prune_stale_importers = may_prune_stale_importers(&StaleImporterPrune {
            filtered_install: importers.filtered_install,
            mutation: install.mutation,
            workspace_projects,
            workspace_projects_are_overridden,
            config: install.config,
        });
        Self { project_manifests, importers, prune_stale_importers }
    }

    // Optimistic repeat-install short-circuit. When nothing has
    // changed since the previous successful install (settings,
    // workspace structure, manifest mtimes), skip the entire
    // install pipeline and emit pnpm's "Already up to date" log.
    // The fast path runs before any of the install setup (no
    // lockfile reads, no verifier fan-out, no `getContext`).
    //
    // Disabled when `--frozen-lockfile` is requested: an explicit
    // headless install should always go through the dispatch so a
    // `NoLockfile` or `OutdatedLockfile` error still fires when
    // the lockfile is missing or stale.

    // Only a full `pacquet install` may short-circuit. `add` and
    // `remove` mutate the manifest in memory and persist it after
    // this run returns, so the on-disk mtimes the check reads still
    // describe the pre-mutation project — without this gate a fresh
    // workspace state would read as "nothing changed → already up
    // to date" and the mutation would never be resolved or
    // materialized. `pacquet update` is
    // excluded through its seed policy: a compatible bump leaves
    // the manifest byte-identical, which the check would likewise
    // read as up to date and skip the registry re-resolution.
    //
    // A `--filter` narrowing does not disqualify the run: the check
    // validates the whole workspace (`project_manifests` covers every
    // project even when only a subset is selected), and it refuses a
    // workspace state a filtered install wrote, so "nothing changed"
    // still means every selected project is materialized.
    pub(super) fn is_already_up_to_date<Reporter: self::Reporter>(
        &self,
        install: InstallView<'_>,
        owned: &InstallOwned,
        mode: &RunMode,
        workspace: &InstallWorkspace<'_>,
    ) -> Result<bool, InstallError> {
        install_is_already_up_to_date::<Reporter>(&UpToDateCheck {
            config: install.config,
            workspace_root: &workspace.workspace_root,
            node_linker: install.node_linker,
            included: mode.included,
            supported_architectures: owned.supported_architectures.as_ref(),
            project_manifests: &self.project_manifests,
            is_workspace_install: workspace.workspace_manifest.is_some(),
            lockfile: install.lockfile,
            catalogs: &workspace.catalogs,
            mutation: install.mutation,
            update_seed_policy: &owned.update_seed_policy,
            frozen_lockfile: install.frozen_lockfile,
            disable_optimistic_repeat_install: install.disable_optimistic_repeat_install,
            effective_node_version: mode.effective_node_version.as_deref(),
            prefix: &workspace.prefix,
        })
    }
}
/// Whether anything could declare a catalog this install has to resolve
/// against.
pub(super) fn catalog_context_present(
    config: &Config,
    catalogs_override: Option<&super::super::Catalogs>,
    workspace_dir: Option<&PathBuf>,
) -> bool {
    catalogs_override.is_some()
        || config.catalogs.is_some()
        || (!config.ignore_workspace && workspace_dir.is_some())
}
/// Walk every workspace project's `package.json` once, unless the caller
/// supplied the list. A selection carries its own projects, so it walks
/// nothing.
pub(super) fn discovered_workspace_projects(
    has_selection: bool,
    workspace_projects_override: Option<Vec<pnpm_workspace::Project>>,
    workspace_dir: &Path,
    workspace_manifest: Option<&pnpm_workspace::WorkspaceManifest>,
) -> Result<Option<Vec<pnpm_workspace::Project>>, InstallError> {
    if has_selection {
        return Ok(None);
    }
    if let Some(projects) = workspace_projects_override {
        return Ok(Some(projects));
    }
    load_workspace_projects(workspace_dir, workspace_manifest)
        .map_err(InstallError::FindWorkspaceProjects)
}
/// A full install (pnpm's `mutation: "install"`) is the workspace-wide one and
/// counts every project; a partial one (`add`, `update`, `remove`, ...)
/// targets the project it was run in and reports the single-project shape,
/// with no `total`, exactly as pnpm's non-recursive `scopeLogger` call does.
pub(super) fn emit_scope_log<Reporter: self::Reporter>(
    config: &Config,
    mutation: crate::ProjectMutation,
    workspace_projects: Option<&[pnpm_workspace::Project]>,
    workspace_dir: Option<&Path>,
) {
    if !config.shares_one_lockfile() {
        return;
    }
    let workspace_wide = mutation.is_full_install().then_some(workspace_projects).flatten();
    Reporter::emit(&LogEvent::Scope(ScopeLog {
        level: LogLevel::Debug,
        selected: workspace_wide.map_or(1, <[_]>::len),
        total: workspace_wide.map(<[_]>::len),
        workspace_prefix: workspace_dir.map(|dir| dir.to_string_lossy().into_owned()),
    }));
}
/// What decides which projects this install records as importers.
pub(super) struct ProjectManifestScope<'a, 'scope> {
    manifest: &'a PackageManifest,
    selection: Option<&'scope crate::WorkspaceInstallSelection<'a>>,
    workspace_root: &'scope Path,
    workspace_projects: Option<&'a [pnpm_workspace::Project]>,
    root_manifest_as_workspace_root: bool,
    workspace_projects_are_overridden: bool,
    config: &'scope Config,
}
pub(super) fn install_project_manifests<'a>(
    scope: &ProjectManifestScope<'a, '_>,
) -> Vec<(PathBuf, &'a PackageManifest)> {
    if let Some(selection) = scope.selection {
        return build_selected_project_manifests_list(
            scope.manifest,
            selection.all_projects,
            selection.active_manifest_is_standin,
        );
    }
    if scope.root_manifest_as_workspace_root {
        return build_root_importer_project_manifests_list(
            scope.workspace_root,
            scope.manifest,
            None,
        );
    }
    if scope.workspace_projects_are_overridden || !scope.config.shares_one_lockfile() {
        return build_root_importer_project_manifests_list(
            scope.workspace_root,
            scope.manifest,
            // Dedicated per-project lockfiles record a single "." importer per
            // project; sibling projects only feed the `workspace:` resolver,
            // never the importer list.
            scope.config.shares_one_lockfile().then_some(scope.workspace_projects).flatten(),
        );
    }
    build_project_manifests_list(scope.manifest, scope.workspace_projects)
}
/// What decides whether the install may drop importers no project claims.
pub(super) struct StaleImporterPrune<'a> {
    filtered_install: bool,
    mutation: crate::ProjectMutation,
    workspace_projects: Option<&'a [pnpm_workspace::Project]>,
    workspace_projects_are_overridden: bool,
    config: &'a Config,
}
/// Only an install that covers a whole workspace sees the complete project
/// list, so only it may conclude that an importer the lockfile records belongs
/// to a project that is gone. This is pnpm's `pruneLockfileImporters`, which
/// its recursive install defaults to the same condition (`pkgs.length ===
/// allProjects.length`) — outside a workspace there is no project list to
/// compare against. A `NodeApiProject[]` handed in by an API consumer carries
/// no promise of listing every workspace project, so it cannot stand in for
/// the project list either.
pub(super) fn may_prune_stale_importers(prune: &StaleImporterPrune<'_>) -> bool {
    !prune.filtered_install
        && prune.mutation.is_full_install()
        && prune.workspace_projects.is_some()
        && !prune.workspace_projects_are_overridden
        && prune.config.shares_one_lockfile()
}
/// Report the projects this install covers depending on each other in a cycle
/// — after the repeat-install short-circuit, because pnpm returns from
/// "Already up to date" before reaching its own check, and before any
/// resolution, because a `disallowWorkspaceCycles` failure must not be paid
/// for.
pub(super) fn report_install_scope_cycles<Reporter: self::Reporter>(
    config: &Config,
    workspace_dir: Option<&Path>,
    selection: Option<&crate::WorkspaceInstallSelection<'_>>,
    scope: (crate::ProjectMutation, Option<&[pnpm_workspace::Project]>),
) -> Result<(), InstallError> {
    if config.ignore_workspace_cycles {
        return Ok(());
    }
    let Some(workspace_dir) = workspace_dir else { return Ok(()) };
    let (mutation, workspace_projects) = scope;
    let scope = match selection {
        // A plan that already sequenced this very graph hands its cycle report
        // over; the install then skips rebuilding the graph just to find them
        // again.
        Some(selection) => match selection.workspace_cycles {
            crate::PrecomputedWorkspaceCycles::Known(cycles) => {
                return crate::report_workspace_cycles::<Reporter>(config, workspace_dir, cycles)
                    .map_err(InstallError::CyclicWorkspaceDependencies);
            }
            crate::PrecomputedWorkspaceCycles::Unknown => {
                Some((selection.all_projects, Some(selection.selected_dirs)))
            }
        },
        // A single-project mutation (`add`, `update`, ...) has no set to cycle
        // within; only a full install covers the whole workspace.
        None => mutation
            .is_full_install()
            .then_some(workspace_projects)
            .flatten()
            .map(|projects| (projects, None)),
    };
    let Some((projects, selected_dirs)) = scope else { return Ok(()) };
    let cycles = crate::install_scope_cycles(config, projects, selected_dirs);
    crate::report_workspace_cycles::<Reporter>(config, workspace_dir, cycles.as_deref())
        .map_err(InstallError::CyclicWorkspaceDependencies)
}
