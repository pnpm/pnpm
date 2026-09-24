use super::{
    super::{
        Arc, HashSet, InstallError, Lockfile, LogEvent, LogLevel, Path, PathBuf, PnpmLog, Reporter,
        emit_initial_package_manifest, report_merged_lockfile_conflicts,
    },
    HookedManifests, InstallOwned, InstallView, RunMode, resolve_pnpmfile_hook,
    workspace::{InstallScope, InstallWorkspace, report_install_scope_cycles, workspace_projects},
};
use pnpm_config::Config;

/// The lockfiles as loaded, and what the wanted one's load let start: the
/// host probe, the pnpmfile, and the project manifests as its hooks
/// rewrote them.
pub(super) struct Loaded<'a> {
    pub(super) wanted: LoadedWantedLockfile<'a>,
    pub(super) current: Option<Lockfile>,
    pub(super) early_host_detection:
        Option<pnpm_deps_restorer::materialization_plan::HostDetection>,
    pub(super) pnpmfile_hook: Option<Arc<dyn pnpm_hooks::PnpmfileHooks>>,
    pub(super) manifests: HookedManifests,
}
/// Consumes the pnpmfile override off `owned`.
pub(super) async fn load_lockfiles<'a, Reporter: self::Reporter + 'static>(
    install: InstallView<'a>,
    owned: &mut InstallOwned,
    mode: &RunMode,
    workspace: &InstallWorkspace<'a>,
    scope: &InstallScope<'a>,
    selection: Option<&crate::WorkspaceInstallSelection<'_>>,
    discovery: (&HashSet<PathBuf>, Option<&[pnpm_workspace::Project]>),
) -> Result<Loaded<'a>, InstallError> {
    let (pre_hooked_paths, loaded_workspace_projects) = discovery;
    let StartedLockfiles {
        wanted,
        current_lockfile_task,
        early_host_detection,
    } = start_lockfile_load::<Reporter>(
        install,
        owned,
        mode,
        workspace,
        selection,
        loaded_workspace_projects,
    )?;
    announce_manifest_load::<Reporter>(install, &workspace.dirs.workspace_root);
    let (pnpmfile_hook, manifests) =
        load_hooked_manifests::<Reporter>(install, owned, workspace, scope, pre_hooked_paths)
            .await?;
    Ok(Loaded {
        wanted,
        current: join_current_lockfile_load::<Reporter>(
            current_lockfile_task,
            install.context.config,
            &workspace.prefix,
        )
        .await,
        early_host_detection,
        pnpmfile_hook,
        manifests,
    })
}
pub(super) type CurrentLockfileLoad =
    tokio::task::JoinHandle<Result<Option<Lockfile>, pnpm_lockfile::LoadLockfileError>>;
pub(super) struct StartedLockfiles<'a> {
    wanted: LoadedWantedLockfile<'a>,
    current_lockfile_task: CurrentLockfileLoad,
    early_host_detection: Option<pnpm_deps_restorer::materialization_plan::HostDetection>,
}
// Publish the initial manifest before the context event that renders the install header.
pub(super) fn announce_manifest_load<Reporter: self::Reporter>(
    install: InstallView<'_>,
    workspace_root: &Path,
) {
    register_workspace_in_store(install.context.config, workspace_root);
    if install.context.emit_initial_manifest {
        emit_initial_package_manifest::<Reporter>(install.context.manifest);
    }
}
// Overlap the wanted-lockfile prefetch with cycle validation, then start the independent current read.
pub(super) fn start_lockfile_load<'a, Reporter: self::Reporter>(
    install: InstallView<'a>,
    owned: &InstallOwned,
    mode: &RunMode,
    workspace: &InstallWorkspace<'a>,
    selection: Option<&crate::WorkspaceInstallSelection<'_>>,
    loaded_workspace_projects: Option<&[pnpm_workspace::Project]>,
) -> Result<StartedLockfiles<'a>, InstallError> {
    install.context.lockfile.prefetch();
    // Report the projects this install covers depending on each
    // other in a cycle — after the short-circuit above, because pnpm
    // returns from "Already up to date" before reaching its own
    // check, and before any resolution, because a
    // `disallowWorkspaceCycles` failure must not be paid for.
    report_install_scope_cycles::<Reporter>(
        install.context.config,
        workspace,
        selection,
        (install.execution.mutation, workspace_projects(loaded_workspace_projects, selection)),
    )?;
    let current_lockfile_task = spawn_current_lockfile_load(install.context.config);
    // Past the repeat-install fast path every install flavor needs
    // the wanted lockfile's contents; force the deferred load here.
    // A broken lockfile is regenerable state, so only a frozen
    // install treats it as fatal (upstream `readLockfiles`).
    let phase_start = std::time::Instant::now();
    let wanted = load_wanted_lockfile::<Reporter>(
        install.context.lockfile,
        install.lockfile_policy.frozen,
        (&workspace.dirs.workspace_root, &workspace.prefix),
    )?;
    tracing::info!(
        target: "pacquet::install::phase",
        phase = "load_wanted_lockfile",
        elapsed_ms = phase_start.elapsed().as_millis() as u64,
        "phase complete",
    );
    // Spawn the installability host detection (`node --version`,
    // ~150 ms of node startup) as soon as the wanted lockfile is
    // parsed, so the probe overlaps planning on the frozen path and
    // the whole resolution on the fresh path.
    let early_host_detection =
        needs_early_host_detection(install.context.config, mode.resolve_only, wanted.lockfile)
            .then(|| {
                pnpm_deps_restorer::materialization_plan::HostDetection::spawn(
                    install.context.config.effective_engine_strict(),
                    mode.effective_node_version.clone(),
                    owned.projects.supported_architectures.clone(),
                )
            });
    Ok(StartedLockfiles { wanted, current_lockfile_task, early_host_detection })
}
// Both lockfiles can be megabyte-scale YAML documents; read the current one off the reactor
// while the wanted one parses, since neither depends on the other.
pub(super) fn spawn_current_lockfile_load(config: &Config) -> CurrentLockfileLoad {
    let virtual_store_dir = config.virtual_store_dir.clone();
    tokio::task::spawn_blocking(move || {
        Lockfile::load_current_from_virtual_store_dir(&virtual_store_dir)
    })
}
// Load the *current* lockfile that records what the previous
// install actually materialized in `<virtual_store_dir>/lock.yaml`.
// The frozen-lockfile path diffs each wanted snapshot against
// this on a per-`PackageKey` basis to decide whether the
// already-installed slot is still usable. `Ok(None)` on a
// first install (the file doesn't exist yet). A corrupted /
// version-incompatible file is disposable state: pnpm warns and
// continues with an empty current lockfile because the wanted
// lockfile and filesystem remain authoritative.
pub(super) async fn join_current_lockfile_load<Reporter: self::Reporter>(
    task: CurrentLockfileLoad,
    config: &Config,
    prefix: &str,
) -> Option<Lockfile> {
    let phase_start = std::time::Instant::now();
    let current_lockfile = load_current_lockfile::<Reporter>(
        task.await.expect("join the current-lockfile load task"),
        config,
        prefix,
    );
    tracing::info!(
        target: "pacquet::install::phase",
        phase = "load_current_lockfile",
        elapsed_ms = phase_start.elapsed().as_millis() as u64,
        "phase complete",
    );
    current_lockfile
}
/// The wanted lockfile's contents, with the loader's handles onto the same
/// document. A broken lockfile is regenerable state, so only a frozen install
/// treats it as fatal (upstream `readLockfiles`).
///
/// The fold's "before" is read out here rather than at its use site: a load
/// that failed leaves nothing cached, so asking later would retry it and turn
/// a lockfile this arm chose to ignore into a fatal one.
/// The wanted lockfile as its loader holds it: the document, the loader's
/// shared handle to it, the intact copy a filtered install splices back
/// over, and the importers as they were before the branch lockfiles were
/// folded in.
#[derive(Default)]
pub(super) struct LoadedWantedLockfile<'a> {
    pub(super) lockfile: Option<&'a Lockfile>,
    pub(super) shared: Option<Arc<Lockfile>>,
    pub(super) merge: Option<&'a Lockfile>,
    pub(super) pre_merge_importers:
        Option<&'a std::collections::HashMap<String, pnpm_lockfile::ProjectSnapshot>>,
    /// Whether the file only parsed because its Git conflict markers were
    /// merged away. The file on disk still holds them, so the install may
    /// not settle on a path that leaves it unwritten.
    pub(super) had_conflicts: bool,
}
pub(super) fn load_wanted_lockfile<'a, Reporter: self::Reporter>(
    lockfile_source: super::super::MaybeLazyLockfile<'a>,
    frozen_lockfile: bool,
    context: (&Path, &str),
) -> Result<LoadedWantedLockfile<'a>, InstallError> {
    let (workspace_root, prefix) = context;
    match lockfile_source.get() {
        Ok(lockfile) => {
            let merged_conflict_files = lockfile_source
                .merged_conflict_files()
                .map_err(InstallError::LoadWantedLockfile)?;
            report_merged_lockfile_conflicts::<Reporter>(merged_conflict_files, prefix);
            Ok(LoadedWantedLockfile {
                lockfile,
                shared: lockfile_source.shared().map_err(InstallError::LoadWantedLockfile)?,
                merge: lockfile_source.get_for_merge().map_err(InstallError::LoadWantedLockfile)?,
                pre_merge_importers: lockfile_source
                    .pre_merge_importers()
                    .map_err(InstallError::LoadWantedLockfile)?,
                had_conflicts: merged_conflict_files > 0,
            })
        }
        Err(error) if !frozen_lockfile => {
            Reporter::emit(&LogEvent::Pnpm(PnpmLog {
                level: LogLevel::Warn,
                message: format!(
                    "Ignoring broken lockfile at {}: {error}",
                    workspace_root.display(),
                ),
                prefix: prefix.to_string(),
            }));
            Ok(LoadedWantedLockfile::default())
        }
        Err(error) => Err(InstallError::LoadWantedLockfile(error)),
    }
}
/// A constraint-free lockfile spawns nothing — the probe's result would go
/// unused (see `detect_installability_host` for why that matters) — and
/// neither does `--force` (skips the checks) or a resolve-only pass (returns
/// before them). The scan is of the *wanted* lockfile: a fresh resolve whose
/// new graph gains constraints the old lockfile lacked detects the host at
/// its own site.
pub(super) fn needs_early_host_detection(
    config: &Config,
    resolve_only: bool,
    lockfile: Option<&Lockfile>,
) -> bool {
    !config.installs_incompatible_packages()
        && !resolve_only
        && lockfile.is_some_and(|lockfile| match (&lockfile.snapshots, &lockfile.packages) {
            (Some(snapshots), Some(packages)) if !snapshots.is_empty() => {
                pnpm_deps_restorer::any_installability_constraint(snapshots, packages)
            }
            _ => false,
        })
}
/// Register the workspace root in the store's project registry, once per
/// install. Store prune walks the workspace's `node_modules/.pnpm/` to find
/// every installed package, so one entry per workspace is enough.
///
/// Gated on `enableGlobalVirtualStore` because pacquet wires the
/// prune-by-registry path only under GVS for now; pnpm registers
/// unconditionally, so once the non-GVS prune path lands the gate should be
/// dropped.
///
/// Best-effort: a registry write failure shouldn't fail the install, so it is
/// surfaced as `tracing::warn!` instead.
pub(super) fn register_workspace_in_store(config: &Config, workspace_root: &Path) {
    if !config.enable_global_virtual_store {
        return;
    }
    // Create the store root before calling `register_project` so its
    // `path_contains` guard can canonicalize the path instead of falling
    // through to a literal comparison that wrongly matches against
    // `<workspace>/../pacquet-store/v11`-shaped relative store paths
    // (resolved-on-disk: outside the workspace; lexical: starts with the
    // workspace prefix).
    if let Err(error) = std::fs::create_dir_all(pnpm_store_dir::StoreDir::root(&config.store_dir)) {
        tracing::warn!(
            target: "pacquet::install",
            ?error,
            "Failed to ensure store root exists before project registry write; install continues",
        );
    }
    if let Err(error) = pnpm_store_dir::register_project(&config.store_dir, workspace_root) {
        tracing::warn!(
            target: "pacquet::install",
            ?error,
            "Failed to register workspace root in the store project registry; install continues",
        );
    }
}
/// A corrupted or version-incompatible current lockfile is disposable state:
/// pnpm warns and continues with none, because the wanted lockfile and
/// filesystem remain authoritative.
pub(super) fn load_current_lockfile<Reporter: self::Reporter>(
    loaded: Result<Option<Lockfile>, pnpm_lockfile::LoadLockfileError>,
    config: &Config,
    prefix: &str,
) -> Option<Lockfile> {
    match loaded {
        Ok(lockfile) => lockfile,
        Err(error) => {
            Reporter::emit(&LogEvent::Pnpm(PnpmLog {
                level: LogLevel::Warn,
                message: format!(
                    "Ignoring broken lockfile at {}: {error}",
                    config.virtual_store_dir.display(),
                ),
                prefix: prefix.to_string(),
            }));
            None
        }
    }
}

async fn load_hooked_manifests<Reporter: self::Reporter + 'static>(
    install: InstallView<'_>,
    owned: &mut InstallOwned,
    workspace: &InstallWorkspace<'_>,
    scope: &InstallScope<'_>,
    pre_hooked_paths: &HashSet<PathBuf>,
) -> Result<(Option<Arc<dyn pnpm_hooks::PnpmfileHooks>>, HookedManifests), InstallError> {
    // The pnpmfile whose checksum the freshness gates compare
    // against a lockfile's `pnpmfileChecksum`, resolved the way the
    // install that records one resolves it. Building the handle
    // costs a `stat`. The Node worker only starts if a gate has to
    // ask whether the pnpmfile exports hooks. The handle is handed to
    // the resolve path below so an install spawns at most one.
    let pnpmfile_hook = resolve_pnpmfile_hook(
        install.context.config,
        &workspace.dirs.workspace_root,
        owned.projects.pnpmfile_hook_override.take(),
    )?;
    let manifests = HookedManifests::hook::<Reporter>(
        install.context.config,
        &workspace.dirs.workspace_root,
        &scope.project_manifests,
        pnpmfile_hook.as_ref(),
        pre_hooked_paths,
    )
    .await?;
    Ok((pnpmfile_hook, manifests))
}
