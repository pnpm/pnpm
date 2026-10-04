use super::{
    super::{
        HashSet, InstallError, InstallRunOptions, LogEvent, LogLevel, PackageManifest, Path,
        PathBuf, Reporter, ScopeLog, build_project_manifests_list,
        build_root_importer_project_manifests_list, build_selected_project_manifests_list,
        build_workspace_packages_map, configured_or_discovered_workspace_dir,
        get_catalogs_from_workspace_manifest, load_workspace_projects, lockfile_root_dir,
    },
    InstallOwned, InstallView, RepeatInstallVerdict, RunMode, UpToDateCheck,
    repeat_install_verdict,
};
use pnpm_config::Config;

/// The directories the run anchors on.
pub(super) struct WorkspaceDirs<'a> {
    pub(super) manifest_dir: &'a Path,
    pub(super) workspace_dir: Option<PathBuf>,
    pub(super) workspace_manifest_dir: PathBuf,
    pub(super) workspace_root: PathBuf,
}
impl<'a> WorkspaceDirs<'a> {
    fn read_manifest(&self) -> Result<Option<pnpm_workspace::WorkspaceManifest>, InstallError> {
        self.workspace_dir
            .as_deref()
            .map(pnpm_workspace::read_workspace_manifest)
            .transpose()
            .map_err(InstallError::ReadWorkspaceManifest)
            .map(Option::flatten)
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
        let manifest_dir = install.context.manifest
            .path()
            .parent()
            .expect("manifest path always has a parent dir");
        let workspace_dir =
            configured_or_discovered_workspace_dir(install.context.config, manifest_dir)
                .map_err(InstallError::FindWorkspaceDir)?;
        Ok(Self {
            manifest_dir,
            workspace_manifest_dir: workspace_dir
                .clone()
                .unwrap_or_else(|| manifest_dir.to_path_buf()),
            // Catalogs and workspace packages still come from the real
            // workspace dir, which `lockfile_root_dir` parts ways with
            // under `sharedWorkspaceLockfile: false`.
            workspace_root: lockfile_root_dir(install.context.config, manifest_dir)
                .map_err(InstallError::FindWorkspaceDir)?,
            workspace_dir,
        })
    }
}
/// The workspace the run installs into: the directories it anchors on,
/// its catalogs and its projects.
pub(super) struct InstallWorkspace<'a> {
    workspace_manifest: Option<pnpm_workspace::WorkspaceManifest>,
    pub(super) catalog_context_present: bool,
    pub(super) catalogs: super::super::Catalogs,
    pub(super) prefix: String,
    pub(super) workspace_projects_are_overridden: bool,
    pub(super) loaded_workspace_projects: Option<Vec<pnpm_workspace::Project>>,
    pub(super) workspace_packages: Option<pnpm_resolving_resolver_base::WorkspacePackages>,
    pub(super) dirs: WorkspaceDirs<'a>,
}
/// The projects the run installs, and how a selection narrows them.
pub(super) struct InstallScope<'w> {
    pub(super) project_manifests: Vec<(PathBuf, &'w PackageManifest)>,
    pub(super) importers: ImporterSelection,
    pub(super) prune_stale_importers: bool,
    /// A `--frozen-lockfile` repeat install found nothing changed, so an
    /// up-to-date tree runs no project lifecycle scripts.
    pub(super) project_scripts_current: bool,
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
            requested_importer_ids: filtered_install
                .then_some(selection)
                .flatten()
                .map(|selection| {
                    importer_ids(
                        workspace_root,
                        selection.install_dirs.iter().map(PathBuf::as_path),
                    )
                }),
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
            install.context.config,
            owned.projects.catalogs_override.as_ref(),
            dirs.workspace_dir.as_ref(),
        );
        let catalogs = resolve_install_catalogs(
            install.context.config,
            owned.projects.catalogs_override.take(),
            workspace_manifest.as_ref(),
        )?;
        let workspace_projects_are_overridden = owned.projects
            .workspace_projects_override
            .is_some();
        let loaded_workspace_projects = discovered_workspace_projects(
            options.selection.is_some(),
            owned.projects.workspace_projects_override.take(),
            dirs.workspace_dir.as_deref().unwrap_or(&dirs.workspace_root),
            workspace_manifest.as_ref(),
            &install.context.config.managed_directories(),
        )?;
        report_discovered_scope::<Reporter>(
            install,
            options,
            &dirs,
            loaded_workspace_projects.as_deref(),
        );
        let workspace_packages =
            workspace_packages_for_install(install, loaded_workspace_projects.as_deref(), options);
        Ok(Self {
            // Use `to_string_lossy` rather than `to_str().expect(...)` so a
            // valid filesystem path with non-UTF-8 bytes (possible on Unix)
            // doesn't panic the installer. `prefix` is used only for
            // reporter envelopes, so a lossy conversion is acceptable —
            // the rest of the install path uses the same pattern for
            // paths threaded into log events.
            prefix: dirs.workspace_root.to_string_lossy().into_owned(),
            workspace_manifest,
            catalog_context_present,
            catalogs,
            workspace_projects_are_overridden,
            loaded_workspace_projects,
            workspace_packages,
            dirs,
        })
    }
}
fn workspace_packages_for_install<'s>(
    install: InstallView<'_>,
    loaded_workspace_projects: Option<&'s [pnpm_workspace::Project]>,
    options: &InstallRunOptions<'s, '_>,
) -> Option<pnpm_resolving_resolver_base::WorkspacePackages> {
    (install.context.config.exclude_links_from_lockfile
        && install.context.config.link_workspace_packages.enabled_at_depth(0))
    .then(|| {
        build_workspace_packages_map(workspace_projects(
            loaded_workspace_projects,
            options.selection.as_ref(),
        ))
    })
    .flatten()
}
// In-memory mutation catalogs take precedence over hooked configuration and the workspace file.
// Filtered and dedicated-lockfile installs have already reported their own scope.
pub(super) fn report_discovered_scope<Reporter: self::Reporter>(
    install: InstallView<'_>,
    options: &InstallRunOptions<'_, '_>,
    dirs: &WorkspaceDirs,
    loaded_workspace_projects: Option<&[pnpm_workspace::Project]>,
) {
    let workspace_projects = options.selection
        .as_ref()
        .map_or_else(|| loaded_workspace_projects, |selection| Some(selection.all_projects));
    if options.selection.is_none() {
        emit_scope_log::<Reporter>(
            install.context.config,
            install.execution.mutation,
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
            manifest: install.context.manifest,
            selection: options.selection.as_ref(),
            workspace_root,
            workspace_projects,
            root_manifest_as_workspace_root: options.root_manifest_as_workspace_root,
            workspace_projects_are_overridden,
            config: install.context.config,
        });
        let importers = ImporterSelection::select(
            options.selection.as_ref(),
            workspace_root,
            &project_manifests,
        );
        let prune_stale_importers = may_prune_stale_importers(&StaleImporterPrune {
            filtered_install: importers.filtered_install,
            mutation: install.execution.mutation,
            workspace_projects,
            workspace_projects_are_overridden,
            config: install.context.config,
        });
        Self { project_manifests, importers, prune_stale_importers, project_scripts_current: false }
    }

    pub(super) fn repeat_install_verdict(
        &self,
        install: InstallView<'_>,
        owned: &InstallOwned,
        mode: &RunMode,
        workspace: &InstallWorkspace<'_>,
    ) -> Result<RepeatInstallVerdict, InstallError> {
        repeat_install_verdict(&UpToDateCheck {
            workspace: super::super::OptimisticRepeatInstallCheck {
                config: install.context.config,
                workspace_root: &workspace.dirs.workspace_root,
                project_manifests: &self.project_manifests,
                is_workspace_install: workspace.workspace_manifest.is_some(),
                lockfile: install.context.lockfile,
                catalogs: &workspace.catalogs,
                layout: crate::RepeatInstallLayout {
                    node_linker: install.execution.node_linker,
                    included: mode.included,
                    supported_architectures: owned.projects
                        .supported_architectures
                        .as_ref(),
                },
                manifest_freshness: install.lockfile_policy.manifest_freshness,
            },
            mutation: install.execution.mutation,
            update_seed_policy: &owned.resolution.update_seed_policy,
            frozen_lockfile: install.lockfile_policy.frozen,
            resolve_only: mode.resolve_only,
            disable_optimistic_repeat_install: install.lockfile_policy.disable_optimistic_repeat,
            effective_node_version: mode.effective_node_version.as_deref(),
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
    ignored_directories: &[PathBuf],
) -> Result<Option<Vec<pnpm_workspace::Project>>, InstallError> {
    if has_selection {
        return Ok(None);
    }
    if let Some(projects) = workspace_projects_override {
        return Ok(Some(projects));
    }
    load_workspace_projects(workspace_dir, workspace_manifest, ignored_directories)
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
    let workspace_wide = mutation
        .is_full_install()
        .then_some(workspace_projects)
        .flatten();
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
            scope.config
                .shares_one_lockfile()
                .then_some(scope.workspace_projects)
                .flatten(),
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
    workspace: &InstallWorkspace<'_>,
    selection: Option<&crate::WorkspaceInstallSelection<'_>>,
    scope: (crate::ProjectMutation, Option<&[pnpm_workspace::Project]>),
) -> Result<(), InstallError> {
    if config.ignore_workspace_cycles {
        return Ok(());
    }
    let Some(workspace_dir) = workspace.dirs.workspace_dir.as_deref() else { return Ok(()) };
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
    let catalogs = pnpm_workspace_projects_graph::WorkspaceCatalogs {
        catalogs: &workspace.catalogs,
        workspace_dir,
    };
    let cycles = crate::install_scope_cycles(config, projects, selected_dirs, Some(catalogs));
    crate::report_workspace_cycles::<Reporter>(config, workspace_dir, cycles.as_deref())
        .map_err(InstallError::CyclicWorkspaceDependencies)
}
