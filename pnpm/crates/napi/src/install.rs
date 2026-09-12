//! `install` / `rebuild` / `getPeerDependencyIssues`.
//!
//! [`install`] runs `pnpm_package_manager::Install` against caller-supplied
//! in-memory manifests. pacquet's install pipeline holds borrowed state
//! (`&'a PackageManifest`, `&'a ResolvedPackages`) and a `&'static Config`, and
//! the CLI drives it from a dedicated 32 MiB-stack thread with its own tokio
//! runtime. The Node API mirrors that: the whole install runs on a worker
//! thread that owns `State` on its stack, and the napi async fn awaits the
//! result over a oneshot channel — so the borrows never have to cross the FFI
//! boundary or become `'static`.
//!
//! [`rebuild`] takes the frozen path against the already-materialized
//! `node_modules`; [`get_peer_dependency_issues`] runs a sink-driven
//! `dry_run` resolve that writes nothing and returns the per-importer
//! peer-dependency issues.

pub(super) mod overlay;

pub use options::{
    InstallOptions, NetworkConfigInput, NodeApiProject, PackageExtensionInput, PeerIssuesOptions,
    ProxyConfigInput,
};
pub use peer_issues::get_peer_dependency_issues;

use std::{
    collections::{BTreeSet, HashMap},
    net::IpAddr,
    path::{Path, PathBuf},
    sync::{Arc, OnceLock},
};

use indexmap::IndexMap;
use napi_derive::napi;
use pnpm_hooks::PnpmfileHooks;
use pnpm_lockfile::{LazyLockfile, MaybeLazyLockfile};
use pnpm_network::{NoProxySetting, ProxyConfig, ThrottledClient, TlsConfig};
use pnpm_package_manager::{
    DepsRequiringBuildSink, Install, ProjectMutation, RebuildOptions, ResolvedPackages,
    UpdateSeedPolicy,
};
use pnpm_package_manifest::{DependencyGroup, PackageManifest};
use pnpm_tarball::MemCache;
use tokio::sync::Mutex;

use crate::{
    config::{ConfigOverlay, resolve_config},
    error::{invalid_manifest_error, to_napi_error, unsupported_option_error},
    hooks::{BatchHookSink, HookSink, JsBatchedReadPackageHook, JsReadPackageHook},
    native_reporter::{NativeRenderer, OutputSink, ReporterOptions},
    reporter_bridge::{EngineCallGuard, LogSink, NodeBridgeReporter, begin_stats, take_stats},
};

/// Per-project add/remove counts. `linkedToRoot` mirrors pnpm's field; pacquet
/// does not emit it separately, so it stays 0 and consumers use
/// `added + removed` for "did anything change".
#[napi(object)]
pub struct InstallStatsResult {
    pub added: f64,
    pub removed: f64,
    pub linked_to_root: f64,
}

/// Result of [`install`]. Mirrors [`InstallResult`] in `index.d.ts`.
#[napi(object)]
pub struct InstallResult {
    pub stats: InstallStatsResult,
    pub deps_requiring_build: Option<Vec<String>>,
    pub store_dir: String,
}

/// The renderer for one call, built on the caller's thread: a
/// `ThreadsafeFunction` must be constructed while the napi environment is
/// live, before the work moves to the dedicated engine thread.
fn build_renderer(
    options: &InstallOptions,
    on_output: Option<OutputSink>,
) -> Option<NativeRenderer> {
    options.reporter.as_ref().map(|reporter| NativeRenderer::new(reporter, &options.dir, on_output))
}

/// Serializes every engine call that touches the process-global log sink /
/// stats accumulator (`install`, `rebuild`, `pack`) so their reporter state
/// never overlaps. Held across the whole call; different install dirs still run
/// one at a time, matching Bit's per-directory `installsRunning` serialization
/// conservatively.
pub fn engine_call_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

#[napi]
pub async fn install(
    options: InstallOptions,
    on_log: Option<LogSink>,
    read_package_hook: Option<HookSink>,
    read_package_batch_hook: Option<BatchHookSink>,
    on_output: Option<OutputSink>,
) -> napi::Result<InstallResult> {
    let _guard = engine_call_lock().lock().await;
    let renderer = build_renderer(&options, on_output);
    let (tx, rx) = tokio::sync::oneshot::channel();
    // Match pacquet's CLI: the synchronous install call chain is deep
    // enough to overflow a default stack on some platforms.
    let worker = std::thread::Builder::new()
        .name("pnpm-napi-install".to_string())
        .stack_size(32 * 1024 * 1024);
    let spawned = worker.spawn(move || {
        let _ = tx.send(run_install_blocking(
            &options,
            on_log,
            renderer,
            read_package_hook,
            read_package_batch_hook,
        ));
    });
    spawned.map_err(|error| {
        napi::Error::from_reason(format!("failed to spawn install thread: {error}"))
    })?;
    rx.await.map_err(|_| napi::Error::from_reason("install worker thread panicked"))?
}

fn run_install_blocking(
    options: &InstallOptions,
    on_log: Option<LogSink>,
    renderer: Option<NativeRenderer>,
    read_package_hook: Option<HookSink>,
    read_package_batch_hook: Option<BatchHookSink>,
) -> napi::Result<InstallResult> {
    // Restores the previous sink and renderer and clears stats on drop —
    // including on a panic in `run_install_inner`, which unwinds this
    // dedicated thread.
    let _sink_guard = EngineCallGuard::with_renderer(on_log, renderer);
    // The batch sink (synthesized by the `@pnpm/napi` wrapper) wins over the
    // per-manifest sink: one threadsafe call serves a whole batch, where
    // per-manifest dispatch pays roughly one event-loop tick per call. The
    // per-manifest sink stays as the fallback for a wrapper that predates
    // the batch contract.
    let pnpmfile_hook: Option<Arc<dyn PnpmfileHooks>> = match read_package_batch_hook {
        Some(batch) => Some(Arc::new(JsBatchedReadPackageHook::new(batch))),
        None => read_package_hook
            .map(|sink| Arc::new(JsReadPackageHook::new(sink)) as Arc<dyn PnpmfileHooks>),
    };
    begin_stats();
    let deps_requiring_build_sink = (options.return_list_of_deps_requiring_build == Some(true))
        .then(DepsRequiringBuildSink::default);
    let outcome = run_install_inner(
        options,
        pnpmfile_hook,
        EngineMode::Install(deps_requiring_build_sink.as_ref().map(Arc::clone)),
    );
    let stats = take_stats();
    let store_dir = outcome?;
    let deps_requiring_build =
        take_deps_requiring_build(deps_requiring_build_sink.as_ref(), stats.deps_requiring_build);
    Ok(InstallResult {
        stats: InstallStatsResult {
            added: stats.added as f64,
            removed: stats.removed as f64,
            linked_to_root: 0.0,
        },
        deps_requiring_build,
        store_dir,
    })
}

/// Take [`InstallResult::deps_requiring_build`] out of the sink the
/// embedder asked for with `returnListOfDepsRequiringBuild`.
///
/// An empty list is a real answer and stays `Some`; a run that computed no
/// list yields `None`, so the embedder keeps its own record. See
/// [`DepsRequiringBuildSink`] for which runs compute one.
///
/// Without a sink the result carries `ignored_builds`, the blocked builds
/// accumulated from `pnpm:ignored-scripts` events.
fn take_deps_requiring_build(
    sink: Option<&DepsRequiringBuildSink>,
    ignored_builds: Vec<String>,
) -> Option<Vec<String>> {
    match sink {
        Some(sink) => sink
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
            .map(|deps| deps.into_iter().collect()),
        None => (!ignored_builds.is_empty()).then_some(ignored_builds),
    }
}

/// Which engine operation [`run_install_inner`] performs. Every mode shares
/// the same `State` / `Install` construction. The mode selects the
/// fresh-resolve (`install`) versus frozen rebuild path, and carries the
/// per-mode payload the engine needs.
enum EngineMode {
    /// Plain install, with the out-slot for `returnListOfDepsRequiringBuild`
    /// when the embedder asked for the list.
    Install(Option<DepsRequiringBuildSink>),
    Rebuild(RebuildOptions),
    /// Peer-issue query: a `dry_run` fresh resolve that writes nothing
    /// and collects the per-importer peer-dependency issues into the
    /// sink. Mirrors v11's `getPeerDependencyIssues` (`dryRun: true`,
    /// `forceFullResolution: true`).
    PeerIssues(pnpm_package_manager::PeerIssuesSink),
}

impl EngineMode {
    fn disable_optimistic_repeat_install(&self) -> bool {
        matches!(self, Self::Install(_) | Self::PeerIssues(_))
    }

    fn peer_issues_sink(&self) -> Option<pnpm_package_manager::PeerIssuesSink> {
        match self {
            Self::PeerIssues(sink) => Some(Arc::clone(sink)),
            Self::Install(_) | Self::Rebuild(_) => None,
        }
    }

    fn deps_requiring_build_sink(&self) -> Option<DepsRequiringBuildSink> {
        match self {
            Self::Install(sink) => sink.as_ref().map(Arc::clone),
            Self::Rebuild(_) | Self::PeerIssues(_) => None,
        }
    }
}

/// The install-shape decisions the engine mode and the caller's options
/// settle between them.
struct InstallShape {
    lockfile_only: bool,
    frozen_lockfile: bool,
    prefer_frozen_lockfile: Option<bool>,
    update_seed_policy: UpdateSeedPolicy,
    mutation: ProjectMutation,
}

impl InstallShape {
    fn new(options: &InstallOptions, mode: &EngineMode) -> Self {
        let ignore_package_manifest = ignores_package_manifest(options, mode);
        let update_requested = updates_everything(options, mode, ignore_package_manifest);
        InstallShape {
            lockfile_only: is_lockfile_only(options, mode, ignore_package_manifest),
            frozen_lockfile: is_frozen(options, mode, update_requested, ignore_package_manifest),
            prefer_frozen_lockfile: prefers_frozen(options, mode, update_requested),
            update_seed_policy: if update_requested {
                UpdateSeedPolicy::drop_all()
            } else {
                UpdateSeedPolicy::KeepAll
            },
            mutation: project_mutation(mode, ignore_package_manifest),
        }
    }
    /// In-memory manifests require the full freshness check. Peer queries
    /// resolve without writes and report through their dedicated sink.
    fn configure<'a>(
        self,
        install: Install<'a, Vec<DependencyGroup>>,
        options: &InstallOptions,
        mode: &EngineMode,
        pnpmfile_hook: Option<Arc<dyn PnpmfileHooks>>,
        lockfile_path: &'a Path,
    ) -> Install<'a, Vec<DependencyGroup>> {
        Install {
            lockfile_path: Some(lockfile_path),
            frozen_lockfile: self.frozen_lockfile,
            prefer_frozen_lockfile: self.prefer_frozen_lockfile,
            ignore_manifest_check: options.ignore_package_manifest == Some(true),
            skip_runtimes: false,
            mutation: self.mutation,
            supported_architectures: None,
            lockfile_only: self.lockfile_only,
            // A peer-issue query resolves without writing anything;
            // the sink presence suppresses the CLI dry-run report.
            dry_run: matches!(mode, EngineMode::PeerIssues(_)),
            update_seed_policy: self.update_seed_policy,
            peer_issues_sink: mode.peer_issues_sink(),
            deps_requiring_build_sink: mode.deps_requiring_build_sink(),
            // The optimistic repeat-install fast path uses on-disk
            // manifest mtimes as its freshness signal. NAPI installs use
            // caller-supplied manifests that can change without touching
            // package.json, so they must continue to the lockfile
            // freshness check. Peer-issue queries must always resolve too.
            disable_optimistic_repeat_install: mode.disable_optimistic_repeat_install(),
            pnpmfile_hook_override: pnpmfile_hook,
            workspace_projects_override: build_workspace_projects_override(&options.projects),
            ..install
        }
    }
}

/// `ignorePackageManifest` is pnpm's "install from the lockfile, ignore the
/// project manifests" mode — the shape the `pnpm fetch` handler passes in
/// both stacks. The install takes the frozen path against the lockfile
/// alone (the manifest ↔ lockfile freshness gate is skipped via
/// `ignore_manifest_check`), and `virtualStoreOnly` — forced in
/// [`build_overlay`] — suppresses all post-import linking: importer symlinks,
/// `.bin` entries, hoisting, and project lifecycle scripts. Confined to the
/// install path: a rebuild runs against an already-materialized
/// `node_modules` and must keep its own frozen shape even when the caller
/// reuses install options that carry the flag.
fn ignores_package_manifest(options: &InstallOptions, mode: &EngineMode) -> bool {
    matches!(mode, EngineMode::Install(_)) && options.ignore_package_manifest == Some(true)
}

/// `update: true` re-resolves the whole graph to the highest in-range
/// version — pnpm's `update: true` / `depth: Infinity`. The binding takes no
/// package selectors, so an update always targets every dependency
/// (`UpdateSeedPolicy::drop_all()`); `depth` is only pnpm's direct-vs-any-depth
/// selector toggle, which has no effect without selectors and is accepted for
/// API compatibility only. Mirrors `pnpm_package_manager::Update`, which
/// forces `prefer_frozen_lockfile: false` and a non-frozen path so the
/// re-resolution is not short-circuited by the auto-frozen / repeat-install
/// fast paths. `ignorePackageManifest` contradicts an update — it installs
/// exactly what the lockfile records — and wins, matching pnpm, where it
/// forces the headless path.
fn updates_everything(
    options: &InstallOptions,
    mode: &EngineMode,
    ignore_package_manifest: bool,
) -> bool {
    matches!(mode, EngineMode::Install(_))
        && options.update == Some(true)
        && !ignore_package_manifest
}

/// `enableModulesDir: false` ("do not create a `node_modules` directory") is
/// honored via pacquet's lockfile-only path: the graph resolves and the
/// lockfile is written, but nothing is materialized under `node_modules`.
/// Confined to the install path — a rebuild runs against an
/// already-materialized `node_modules`, so it must never take the
/// lockfile-only short-circuit (which would make it silently do nothing) even
/// when the caller reuses install options that disable the modules dir.
/// `ignorePackageManifest` overrides both: it materializes the virtual
/// store from the lockfile, which the lockfile-only short-circuit would
/// skip entirely (the TS fetch handler forces `enableModulesDir: true` for
/// the same reason).
fn is_lockfile_only(
    options: &InstallOptions,
    mode: &EngineMode,
    ignore_package_manifest: bool,
) -> bool {
    matches!(mode, EngineMode::Install(_))
        && !ignore_package_manifest
        && (options.lockfile_only.unwrap_or(false) || options.enable_modules_dir == Some(false))
}

/// A rebuild takes the frozen path against the already-materialized
/// `node_modules`, and re-runs dependency build scripts rather than the
/// root project's own lifecycle scripts.
fn is_frozen(
    options: &InstallOptions,
    mode: &EngineMode,
    update_requested: bool,
    ignore_package_manifest: bool,
) -> bool {
    match mode {
        EngineMode::Install(_) => {
            ignore_package_manifest
                || (!update_requested && options.frozen_lockfile.unwrap_or(false))
        }
        EngineMode::Rebuild(_) => true,
        // Peer issues need a full fresh resolve — never frozen.
        EngineMode::PeerIssues(_) => false,
    }
}

fn prefers_frozen(
    options: &InstallOptions,
    mode: &EngineMode,
    update_requested: bool,
) -> Option<bool> {
    if update_requested || matches!(mode, EngineMode::PeerIssues(_)) {
        Some(false)
    } else {
        options.prefer_frozen_lockfile
    }
}

/// An `ignorePackageManifest` install materializes what the lockfile
/// records without installing any project's manifest — `NoInstall`, the
/// mutation both stacks' fetch handlers use.
fn project_mutation(mode: &EngineMode, ignore_package_manifest: bool) -> ProjectMutation {
    if matches!(mode, EngineMode::Install(_)) && !ignore_package_manifest {
        ProjectMutation::InstallWorkspace
    } else {
        ProjectMutation::NoInstall
    }
}

fn run_install_inner(
    options: &InstallOptions,
    pnpmfile_hook: Option<Arc<dyn PnpmfileHooks>>,
    mode: EngineMode,
) -> napi::Result<String> {
    reject_non_object_manifests(&options.projects)?;
    let dir = PathBuf::from(&options.dir);
    let manifest =
        PackageManifest::from_value(dir.join("package.json"), root_manifest_value(options, &dir)?);

    reject_unsupported_install_options(options)?;
    let config =
        resolve_config(&dir, &build_overlay(options, ignores_package_manifest(options, &mode))?)
            .map_err(|error| to_napi_error(&error))?;

    let http_client = install_http_client(config)?;
    let lazy_lockfile = if config.lockfile {
        LazyLockfile::deferred(dir.clone(), config.wanted_lockfile_selection())
    } else {
        LazyLockfile::disabled()
    };
    let resolved_packages = ResolvedPackages::new();
    let lockfile_path = dir.join(config.wanted_lockfile_name());
    let shape = InstallShape::new(options, &mode);

    multi_thread_runtime()?
        .block_on(async {
            let install = Install::new(
                Arc::new(MemCache::new()),
                &resolved_packages,
                (&http_client, Arc::clone(&http_client)),
                config,
                &manifest,
                MaybeLazyLockfile::Lazy(&lazy_lockfile),
                dependency_groups(options),
            );
            let install = shape.configure(install, options, &mode, pnpmfile_hook, &lockfile_path);
            match mode {
                EngineMode::Install(_) | EngineMode::PeerIssues(_) => {
                    install.run::<NodeBridgeReporter>().await
                }
                EngineMode::Rebuild(rebuild) => {
                    install.run_rebuild::<NodeBridgeReporter>(rebuild).await
                }
            }
        })
        .map_err(|error| to_napi_error(&error))?;

    Ok(PathBuf::from(config.store_dir.clone()).display().to_string())
}

/// The root importer is the project at `dir`; any others are siblings. A
/// lone project takes the plain (non-workspace) install path; multiple
/// importers are handed to the engine via `workspace_projects_override`.
fn root_manifest_value(options: &InstallOptions, dir: &Path) -> napi::Result<serde_json::Value> {
    options
        .projects
        .iter()
        .find(|project| Path::new(&project.root_dir) == dir)
        .map(|project| project.manifest.clone())
        .ok_or_else(|| {
            napi::Error::from_reason(format!(
                "install options had no project entry for the install dir {}",
                options.dir,
            ))
        })
}

pub(crate) fn install_http_client(
    config: &pnpm_config::Config,
) -> napi::Result<Arc<ThrottledClient>> {
    Ok(Arc::new(
        ThrottledClient::for_installs(
            &config.proxy,
            &config.tls,
            &config.tls_by_uri,
            &config.network_settings(),
        )
        .map_err(|error| to_napi_error(&error))?
        .with_max_sockets_per_host(config.max_sockets),
    ))
}

fn dependency_groups(options: &InstallOptions) -> Vec<DependencyGroup> {
    let mut groups = vec![DependencyGroup::Prod, DependencyGroup::Dev];
    if options.include_optional_deps != Some(false) {
        groups.push(DependencyGroup::Optional);
    }
    groups
}

fn multi_thread_runtime() -> napi::Result<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_multi_thread().enable_all().build().map_err(|error| {
        napi::Error::from_reason(format!("failed to build tokio runtime: {error}"))
    })
}

#[napi]
pub async fn rebuild(
    options: InstallOptions,
    on_log: Option<LogSink>,
    selected_names: Option<Vec<String>>,
    on_output: Option<OutputSink>,
) -> napi::Result<()> {
    let _guard = engine_call_lock().lock().await;
    let renderer = build_renderer(&options, on_output);
    let (tx, rx) = tokio::sync::oneshot::channel();
    let worker = std::thread::Builder::new()
        .name("pnpm-napi-rebuild".to_string())
        .stack_size(32 * 1024 * 1024);
    let spawned = worker.spawn(move || {
        let _ = tx.send(run_rebuild_blocking(&options, on_log, renderer, selected_names));
    });
    spawned.map_err(|error| {
        napi::Error::from_reason(format!("failed to spawn rebuild thread: {error}"))
    })?;
    rx.await.map_err(|_| napi::Error::from_reason("rebuild worker thread panicked"))?
}

fn run_rebuild_blocking(
    options: &InstallOptions,
    on_log: Option<LogSink>,
    renderer: Option<NativeRenderer>,
    selected_names: Option<Vec<String>>,
) -> napi::Result<()> {
    // Restores the previous sink and renderer on drop — including on a
    // panic in `run_install_inner`, which unwinds this dedicated thread.
    let _sink_guard = EngineCallGuard::with_renderer(on_log, renderer);
    // `None` (or an empty list) rebuilds every build-needing package; a
    // non-empty list restricts the rebuild to the matching names / build keys.
    let rebuild_options = RebuildOptions {
        selected_names: selected_names
            .filter(|names| !names.is_empty())
            .map(|names| names.into_iter().collect()),
        // The engine API rebuilds dependencies only; running a workspace
        // project's own deferred scripts is `pnpm rebuild --pending`.
        pending_projects: Vec::new(),
    };
    let outcome = run_install_inner(options, None, EngineMode::Rebuild(rebuild_options));
    outcome.map(|_| ())
}

#[cfg(test)]
mod tests;

use overlay::{build_overlay, build_workspace_projects_override};

mod peer_issues;

mod options;

mod validation;
use validation::{reject_non_object_manifests, reject_unsupported_install_options};
