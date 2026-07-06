//! `install` / `rebuild` / `getPeerDependencyIssues`.
//!
//! [`install`] runs `pacquet_package_manager::Install` against caller-supplied
//! in-memory manifests. pacquet's install pipeline holds borrowed state
//! (`&'a PackageManifest`, `&'a ResolvedPackages`) and a `&'static Config`, and
//! the CLI drives it from a dedicated 32 MiB-stack thread with its own tokio
//! runtime. The Node API mirrors that: the whole install runs on a worker
//! thread that owns `State` on its stack, and the napi async fn awaits the
//! result over a oneshot channel — so the borrows never have to cross the FFI
//! boundary or become `'static`.
//!
//! `rebuild` and `getPeerDependencyIssues` remain stubbed; see the
//! `ERR_PNPM_NAPI_UNIMPLEMENTED` returns.

use std::{
    path::PathBuf,
    sync::{Arc, OnceLock},
};

use napi_derive::napi;
use pacquet_hooks::PnpmfileHooks;
use pacquet_lockfile::{LazyLockfile, Lockfile, MaybeLazyLockfile};
use pacquet_network::{NetworkSettings, ThrottledClient};
use pacquet_package_manager::{Install, RebuildOptions, ResolvedPackages, UpdateSeedPolicy};
use pacquet_package_manifest::{DependencyGroup, PackageManifest};
use pacquet_tarball::MemCache;
use tokio::sync::Mutex;

use crate::{
    config::{ConfigOverlay, resolve_config},
    error::{to_napi_error, unimplemented_error},
    hooks::{HookSink, JsReadPackageHook},
    reporter_bridge::{
        LogSink, NodeBridgeReporter, begin_stats, clear_global_log_sink, set_global_log_sink,
        take_stats,
    },
};

/// One importer: an absolute directory plus its in-memory manifest.
#[napi(object)]
pub struct NodeApiProject {
    pub root_dir: String,
    pub manifest: serde_json::Value,
}

/// Options for [`install`]. Mirrors [`InstallOptions`] in `index.d.ts`; only the
/// fields the engine consumes today are read, the rest are accepted and
/// ignored so the contract stays forward-compatible.
#[napi(object)]
pub struct InstallOptions {
    pub dir: String,
    pub projects: Vec<NodeApiProject>,
    pub store_dir: Option<String>,
    pub cache_dir: Option<String>,
    pub registries: Option<std::collections::HashMap<String, String>>,
    pub node_linker: Option<String>,
    pub hoist_pattern: Option<Vec<String>>,
    pub public_hoist_pattern: Option<Vec<String>>,
    pub overrides: Option<std::collections::HashMap<String, String>>,
    pub package_import_method: Option<String>,
    pub auto_install_peers: Option<bool>,
    pub exclude_links_from_lockfile: Option<bool>,
    pub lockfile_only: Option<bool>,
    pub frozen_lockfile: Option<bool>,
    pub prefer_frozen_lockfile: Option<bool>,
    pub prefer_offline: Option<bool>,
    pub offline: Option<bool>,
    pub virtual_store_dir_max_length: Option<u32>,
    pub peers_suffix_max_length: Option<u32>,
    pub dedupe_peer_dependents: Option<bool>,
    pub dedupe_direct_deps: Option<bool>,
    pub dedupe_injected_deps: Option<bool>,
    pub resolve_peers_from_workspace_root: Option<bool>,
    pub include_optional_deps: Option<bool>,
    pub network_concurrency: Option<u32>,
    pub fetch_timeout: Option<u32>,
    pub user_agent: Option<String>,
    /// Fail the install with `ERR_PNPM_IGNORED_BUILDS` when a dependency build
    /// script is blocked. Defaults to `false` — the install instead reports the
    /// blocked packages in `depsRequiringBuild`, matching how embedders (Bit)
    /// gate builds themselves.
    pub strict_dep_builds: Option<bool>,
    /// Per-package build-script allow-list: `name -> allowed`.
    pub allow_builds: Option<std::collections::HashMap<String, bool>>,
    /// Allow every dependency's build scripts to run.
    pub dangerously_allow_all_builds: Option<bool>,
    /// `peerDependencyRules` — how peer-dependency mismatches are treated.
    pub peer_dependency_rules: Option<PeerDependencyRulesInput>,
    /// Pre-computed `Authorization` header values keyed by nerf-darted registry
    /// URI (`//host/path/`), plus `""` for the default registry.
    pub auth_header_by_uri: Option<std::collections::HashMap<String, String>>,
}

/// `peerDependencyRules` input. Mirrors `PeerDependencyRules` in `index.d.ts`.
#[napi(object)]
pub struct PeerDependencyRulesInput {
    pub ignore_missing: Option<Vec<String>>,
    pub allow_any: Option<Vec<String>>,
    pub allowed_versions: Option<std::collections::HashMap<String, String>>,
}

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

/// Serializes engine calls that collect stats through the process-global
/// accumulator (see `reporter_bridge::begin_stats`). Held across the whole
/// install; different install dirs still run one at a time, matching Bit's
/// per-directory `installsRunning` serialization conservatively.
pub fn install_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

#[napi]
pub async fn install(
    options: InstallOptions,
    on_log: Option<LogSink>,
    read_package_hook: Option<HookSink>,
) -> napi::Result<InstallResult> {
    let _guard = install_lock().lock().await;
    let (tx, rx) = tokio::sync::oneshot::channel();
    std::thread::Builder::new()
        .name("pnpm-napi-install".to_string())
        // Match pacquet's CLI: the synchronous install call chain is deep
        // enough to overflow a default stack on some platforms.
        .stack_size(32 * 1024 * 1024)
        .spawn(move || {
            let _ = tx.send(run_install_blocking(&options, on_log, read_package_hook));
        })
        .map_err(|error| {
            napi::Error::from_reason(format!("failed to spawn install thread: {error}"))
        })?;
    rx.await.map_err(|_| napi::Error::from_reason("install worker thread panicked"))?
}

fn run_install_blocking(
    options: &InstallOptions,
    on_log: Option<LogSink>,
    read_package_hook: Option<HookSink>,
) -> napi::Result<InstallResult> {
    let had_sink = on_log.is_some();
    if let Some(sink) = on_log {
        set_global_log_sink(sink);
    }
    let pnpmfile_hook: Option<Arc<dyn PnpmfileHooks>> = read_package_hook
        .map(|sink| Arc::new(JsReadPackageHook::new(sink)) as Arc<dyn PnpmfileHooks>);
    begin_stats();
    let outcome = run_install_inner(options, pnpmfile_hook, EngineMode::Install);
    let stats = take_stats();
    if had_sink {
        clear_global_log_sink();
    }
    let store_dir = outcome?;
    Ok(InstallResult {
        stats: InstallStatsResult {
            added: stats.added as f64,
            removed: stats.removed as f64,
            linked_to_root: 0.0,
        },
        deps_requiring_build: (!stats.deps_requiring_build.is_empty())
            .then_some(stats.deps_requiring_build),
        store_dir,
    })
}

/// Which engine operation [`run_install_inner`] performs. Both share the same
/// `State` / `Install` construction; the mode selects the fresh-resolve
/// (`install`) versus frozen rebuild path and the two fields that differ.
enum EngineMode {
    Install,
    Rebuild(RebuildOptions),
}

fn run_install_inner(
    options: &InstallOptions,
    pnpmfile_hook: Option<Arc<dyn PnpmfileHooks>>,
    mode: EngineMode,
) -> napi::Result<String> {
    let dir = PathBuf::from(&options.dir);

    // The root importer is the project at `dir`; any others are siblings. A
    // lone project takes the plain (non-workspace) install path; multiple
    // importers are handed to the engine via `workspace_projects_override`.
    let root_manifest_value = options
        .projects
        .iter()
        .find(|project| std::path::Path::new(&project.root_dir) == dir)
        .map(|project| project.manifest.clone())
        .ok_or_else(|| {
            napi::Error::from_reason(format!(
                "install options had no project entry for the install dir {}",
                options.dir,
            ))
        })?;
    let workspace_projects_override = build_workspace_projects_override(&options.projects);

    let overlay = build_overlay(options);
    let config = resolve_config(&dir, &overlay).map_err(|error| to_napi_error(&error))?;

    let manifest = PackageManifest::from_value(dir.join("package.json"), root_manifest_value);
    let http_client = Arc::new(
        ThrottledClient::for_installs(
            &config.proxy,
            &config.tls,
            &config.tls_by_uri,
            &NetworkSettings {
                network_concurrency: config.network_concurrency,
                fetch_timeout: std::time::Duration::from_millis(config.fetch_timeout),
                user_agent: config.user_agent.clone(),
            },
        )
        .map_err(|error| to_napi_error(&error))?,
    );
    let lazy_lockfile = if config.lockfile {
        LazyLockfile::deferred(dir.clone())
    } else {
        LazyLockfile::disabled()
    };
    let resolved_packages = ResolvedPackages::new();
    let tarball_mem_cache = Arc::new(MemCache::new());
    let lockfile_path = manifest.path().parent().map(|parent| parent.join(Lockfile::FILE_NAME));

    let mut groups = vec![DependencyGroup::Prod, DependencyGroup::Dev];
    if options.include_optional_deps != Some(false) {
        groups.push(DependencyGroup::Optional);
    }

    // A rebuild takes the frozen path against the already-materialized
    // `node_modules`, and re-runs dependency build scripts rather than the
    // root project's own lifecycle scripts.
    let frozen_lockfile = match &mode {
        EngineMode::Install => options.frozen_lockfile.unwrap_or(false),
        EngineMode::Rebuild(_) => true,
    };
    let is_full_install = matches!(mode, EngineMode::Install);

    let runtime =
        tokio::runtime::Builder::new_multi_thread().enable_all().build().map_err(|error| {
            napi::Error::from_reason(format!("failed to build tokio runtime: {error}"))
        })?;

    runtime
        .block_on(async {
            let install = Install {
                tarball_mem_cache: Arc::clone(&tarball_mem_cache),
                resolved_packages: &resolved_packages,
                http_client: &http_client,
                http_client_arc: Arc::clone(&http_client),
                config,
                manifest: &manifest,
                lockfile: MaybeLazyLockfile::Lazy(&lazy_lockfile),
                lockfile_path: lockfile_path.as_deref(),
                dependency_groups: groups,
                frozen_lockfile,
                prefer_frozen_lockfile: options.prefer_frozen_lockfile,
                ignore_manifest_check: false,
                skip_runtimes: false,
                trust_lockfile: config.trust_lockfile,
                update_checksums: false,
                is_full_install,
                supported_architectures: None,
                node_linker: config.node_linker,
                lockfile_only: options.lockfile_only.unwrap_or(false),
                dry_run: false,
                update_seed_policy: UpdateSeedPolicy::KeepAll,
                auth_override: None,
                resolution_observer: None,
                catalogs_override: None,
                disable_optimistic_repeat_install: false,
                pnpmfile_hook_override: pnpmfile_hook,
                workspace_projects_override,
            };
            match mode {
                EngineMode::Install => install.run::<NodeBridgeReporter>().await,
                EngineMode::Rebuild(rebuild) => {
                    install.run_rebuild::<NodeBridgeReporter>(rebuild).await
                }
            }
        })
        .map_err(|error| to_napi_error(&error))?;

    Ok(PathBuf::from(config.store_dir.clone()).display().to_string())
}

/// Build the in-memory workspace-projects override from the caller's importer
/// list. `None` for a single importer (the plain, non-workspace install path);
/// otherwise one [`pacquet_workspace::Project`] per importer so `workspace:`
/// specifiers resolve across them and each importer gets its own resolved
/// dependency tree. The root importer (the project at the install dir) is
/// included too — `Install::run` skips its `"."` id for the per-importer
/// manifest list (using `Install.manifest`) but still needs it in the
/// `workspace:`-spec lookup.
fn build_workspace_projects_override(
    projects: &[NodeApiProject],
) -> Option<Vec<pacquet_workspace::Project>> {
    if projects.len() <= 1 {
        return None;
    }
    Some(
        projects
            .iter()
            .map(|project| {
                let root_dir = PathBuf::from(&project.root_dir);
                let manifest = PackageManifest::from_value(
                    root_dir.join("package.json"),
                    project.manifest.clone(),
                );
                pacquet_workspace::Project { root_dir, manifest }
            })
            .collect(),
    )
}

fn build_overlay(options: &InstallOptions) -> ConfigOverlay {
    ConfigOverlay {
        store_dir: options.store_dir.as_ref().map(PathBuf::from),
        cache_dir: options.cache_dir.as_ref().map(PathBuf::from),
        registry: None,
        registries: options.registries.as_ref().map(|map| map.clone().into_iter().collect()),
        node_linker: options.node_linker.as_deref().and_then(parse_node_linker),
        package_import_method: options
            .package_import_method
            .as_deref()
            .and_then(parse_import_method),
        virtual_store_dir_max_length: options.virtual_store_dir_max_length.map(u64::from),
        hoist_pattern: options.hoist_pattern.clone(),
        public_hoist_pattern: options.public_hoist_pattern.clone(),
        overrides: options.overrides.as_ref().map(|map| map.clone().into_iter().collect()),
        auto_install_peers: options.auto_install_peers,
        prefer_offline: options.prefer_offline,
        offline: options.offline,
        lockfile: None,
        prefer_frozen_lockfile: options.prefer_frozen_lockfile,
        dedupe_peer_dependents: options.dedupe_peer_dependents,
        dedupe_direct_deps: options.dedupe_direct_deps,
        dedupe_injected_deps: options.dedupe_injected_deps,
        resolve_peers_from_workspace_root: options.resolve_peers_from_workspace_root,
        peers_suffix_max_length: options.peers_suffix_max_length.map(u64::from),
        network_concurrency: options.network_concurrency.map(|value| value as usize),
        fetch_timeout: options.fetch_timeout.map(u64::from),
        user_agent: options.user_agent.clone(),
        // Embedders gate builds themselves, so default to report-not-fail.
        strict_dep_builds: Some(options.strict_dep_builds.unwrap_or(false)),
        allow_builds: options.allow_builds.clone(),
        dangerously_allow_all_builds: options.dangerously_allow_all_builds,
        peer_dependency_rules: options.peer_dependency_rules.as_ref().map(|rules| {
            crate::config::PeerDependencyRulesOverlay {
                ignore_missing: rules.ignore_missing.clone(),
                allow_any: rules.allow_any.clone(),
                allowed_versions: rules
                    .allowed_versions
                    .as_ref()
                    .map(|map| map.clone().into_iter().collect()),
            }
        }),
        auth_header_by_uri: options.auth_header_by_uri.clone(),
    }
}

fn parse_node_linker(value: &str) -> Option<pacquet_config::NodeLinker> {
    match value {
        "hoisted" => Some(pacquet_config::NodeLinker::Hoisted),
        "isolated" => Some(pacquet_config::NodeLinker::Isolated),
        "pnp" => Some(pacquet_config::NodeLinker::Pnp),
        _ => None,
    }
}

fn parse_import_method(value: &str) -> Option<pacquet_config::PackageImportMethod> {
    match value {
        "auto" => Some(pacquet_config::PackageImportMethod::Auto),
        "hardlink" => Some(pacquet_config::PackageImportMethod::Hardlink),
        "copy" => Some(pacquet_config::PackageImportMethod::Copy),
        "clone" => Some(pacquet_config::PackageImportMethod::Clone),
        _ => None,
    }
}

#[napi]
pub async fn rebuild(
    options: InstallOptions,
    on_log: Option<LogSink>,
    selected_names: Option<Vec<String>>,
) -> napi::Result<()> {
    let _guard = install_lock().lock().await;
    let (tx, rx) = tokio::sync::oneshot::channel();
    std::thread::Builder::new()
        .name("pnpm-napi-rebuild".to_string())
        .stack_size(32 * 1024 * 1024)
        .spawn(move || {
            let _ = tx.send(run_rebuild_blocking(&options, on_log, selected_names));
        })
        .map_err(|error| {
            napi::Error::from_reason(format!("failed to spawn rebuild thread: {error}"))
        })?;
    rx.await.map_err(|_| napi::Error::from_reason("rebuild worker thread panicked"))?
}

fn run_rebuild_blocking(
    options: &InstallOptions,
    on_log: Option<LogSink>,
    selected_names: Option<Vec<String>>,
) -> napi::Result<()> {
    let had_sink = on_log.is_some();
    if let Some(sink) = on_log {
        set_global_log_sink(sink);
    }
    // `None` (or an empty list) rebuilds every build-needing package; a
    // non-empty list restricts the rebuild to the matching names / build keys.
    let rebuild_options = RebuildOptions {
        selected_names: selected_names
            .filter(|names| !names.is_empty())
            .map(|names| names.into_iter().collect()),
    };
    let outcome = run_install_inner(options, None, EngineMode::Rebuild(rebuild_options));
    if had_sink {
        clear_global_log_sink();
    }
    outcome.map(|_| ())
}

#[napi(js_name = "getPeerDependencyIssues")]
pub async fn get_peer_dependency_issues(
    _options: serde_json::Value,
) -> napi::Result<serde_json::Value> {
    Err(unimplemented_error("getPeerDependencyIssues"))
}
