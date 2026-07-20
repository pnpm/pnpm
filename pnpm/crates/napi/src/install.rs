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
//! [`rebuild`] takes the frozen path against the already-materialized
//! `node_modules`; [`get_peer_dependency_issues`] runs a sink-driven
//! `dry_run` resolve that writes nothing and returns the per-importer
//! peer-dependency issues.

use std::{
    collections::{BTreeSet, HashMap},
    net::IpAddr,
    path::PathBuf,
    sync::{Arc, OnceLock},
};

use napi_derive::napi;
use pacquet_hooks::PnpmfileHooks;
use pacquet_lockfile::{LazyLockfile, Lockfile, MaybeLazyLockfile};
use pacquet_network::{NetworkSettings, NoProxySetting, ProxyConfig, ThrottledClient, TlsConfig};
use pacquet_package_manager::{Install, RebuildOptions, ResolvedPackages, UpdateSeedPolicy};
use pacquet_package_manifest::{DependencyGroup, PackageManifest};
use pacquet_tarball::MemCache;
use tokio::sync::Mutex;

use crate::{
    config::{ConfigOverlay, resolve_config},
    error::{invalid_manifest_error, to_napi_error, unsupported_option_error},
    hooks::{HookSink, JsReadPackageHook},
    reporter_bridge::{EngineCallGuard, LogSink, NodeBridgeReporter, begin_stats, take_stats},
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
#[derive(Default)]
pub struct InstallOptions {
    pub dir: String,
    pub projects: Vec<NodeApiProject>,
    pub store_dir: Option<String>,
    pub cache_dir: Option<String>,
    pub registries: Option<HashMap<String, String>>,
    pub auth_config: Option<HashMap<String, String>>,
    pub proxy_config: Option<ProxyConfigInput>,
    pub network_config: Option<NetworkConfigInput>,
    pub node_linker: Option<String>,
    /// `linkWorkspacePackages` — `true` / `false` / `"deep"`. When enabled, a
    /// bare-semver dependency may resolve to a workspace package by name (not
    /// only `workspace:`-prefixed ranges).
    pub link_workspace_packages: Option<serde_json::Value>,
    pub hoist_pattern: Option<Vec<String>>,
    pub public_hoist_pattern: Option<Vec<String>>,
    pub external_dependencies: Option<Vec<String>>,
    pub overrides: Option<HashMap<String, String>>,
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
    pub inject_workspace_packages: Option<bool>,
    pub hoist_workspace_packages: Option<bool>,
    pub enable_modules_dir: Option<bool>,
    pub ignore_package_manifest: Option<bool>,
    pub node_version: Option<String>,
    pub engine_strict: Option<bool>,
    pub minimum_release_age: Option<u32>,
    pub minimum_release_age_exclude: Option<Vec<String>>,
    pub never_built_dependencies: Option<Vec<String>>,
    pub update: Option<bool>,
    pub depth: Option<u32>,
    pub include_optional_deps: Option<bool>,
    pub ignore_scripts: Option<bool>,
    pub network_concurrency: Option<u32>,
    pub fetch_retries: Option<u32>,
    pub fetch_retry_factor: Option<u32>,
    pub fetch_retry_mintimeout: Option<u32>,
    pub fetch_retry_maxtimeout: Option<u32>,
    pub fetch_timeout: Option<u32>,
    pub user_agent: Option<String>,
    /// Fail the install with `ERR_PNPM_IGNORED_BUILDS` when a dependency build
    /// script is blocked. Defaults to `false` — the install instead reports the
    /// blocked packages in `depsRequiringBuild`, matching how embedders (Bit)
    /// gate builds themselves.
    pub strict_dep_builds: Option<bool>,
    /// Per-package build-script allow-list: `name -> allowed`.
    pub allow_builds: Option<HashMap<String, bool>>,
    /// Allow every dependency's build scripts to run.
    pub dangerously_allow_all_builds: Option<bool>,
    /// `peerDependencyRules` — how peer-dependency mismatches are treated.
    pub peer_dependency_rules: Option<PeerDependencyRulesInput>,
    /// Pre-computed `Authorization` header values keyed by nerf-darted registry
    /// URI (`//host/path/`), plus `""` for the default registry.
    pub auth_header_by_uri: Option<HashMap<String, String>>,
    pub pnpm_home_dir: Option<String>,
}

#[napi(object)]
#[expect(clippy::struct_field_names, reason = "fields mirror the JavaScript proxyConfig contract")]
pub struct ProxyConfigInput {
    pub http_proxy: Option<String>,
    pub https_proxy: Option<String>,
    pub no_proxy: Option<serde_json::Value>,
}

#[napi(object)]
pub struct NetworkConfigInput {
    pub ca: Option<serde_json::Value>,
    pub cert: Option<serde_json::Value>,
    pub key: Option<String>,
    pub local_address: Option<String>,
    pub strict_ssl: Option<bool>,
    pub max_sockets: Option<u32>,
    pub network_concurrency: Option<u32>,
    pub fetch_retries: Option<u32>,
    pub fetch_retry_factor: Option<u32>,
    pub fetch_retry_mintimeout: Option<u32>,
    pub fetch_retry_maxtimeout: Option<u32>,
    pub fetch_timeout: Option<u32>,
    pub user_agent: Option<String>,
}

/// `peerDependencyRules` input. Mirrors `PeerDependencyRules` in `index.d.ts`.
#[napi(object)]
pub struct PeerDependencyRulesInput {
    pub ignore_missing: Option<Vec<String>>,
    pub allow_any: Option<Vec<String>>,
    pub allowed_versions: Option<HashMap<String, String>>,
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
) -> napi::Result<InstallResult> {
    let _guard = engine_call_lock().lock().await;
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
    // Restores the previous sink and clears stats on drop — including on a
    // panic in `run_install_inner`, which unwinds this dedicated thread.
    let _sink_guard = EngineCallGuard::new(on_log);
    let pnpmfile_hook: Option<Arc<dyn PnpmfileHooks>> = read_package_hook
        .map(|sink| Arc::new(JsReadPackageHook::new(sink)) as Arc<dyn PnpmfileHooks>);
    begin_stats();
    let outcome = run_install_inner(options, pnpmfile_hook, EngineMode::Install);
    let stats = take_stats();
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
    /// Peer-issue query: a `dry_run` fresh resolve that writes nothing
    /// and collects the per-importer peer-dependency issues into the
    /// sink. Mirrors v11's `getPeerDependencyIssues` (`dryRun: true`,
    /// `forceFullResolution: true`).
    PeerIssues(pacquet_package_manager::PeerIssuesSink),
}

fn run_install_inner(
    options: &InstallOptions,
    pnpmfile_hook: Option<Arc<dyn PnpmfileHooks>>,
    mode: EngineMode,
) -> napi::Result<String> {
    reject_non_object_manifests(&options.projects)?;
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

    reject_unsupported_install_options(options)?;
    let overlay = build_overlay(options)?;
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
        .map_err(|error| to_napi_error(&error))?
        .with_max_sockets_per_host(config.max_sockets),
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

    // `update: true` re-resolves the whole graph to the highest in-range
    // version — pnpm's `update: true` / `depth: Infinity`. The binding takes no
    // package selectors, so an update always targets every dependency
    // (`UpdateSeedPolicy::DropAll`); `depth` is only pnpm's direct-vs-any-depth
    // selector toggle, which has no effect without selectors and is accepted for
    // API compatibility only. Mirrors `pacquet_package_manager::Update`, which
    // forces `prefer_frozen_lockfile: false` and a non-frozen path so the
    // re-resolution is not short-circuited by the auto-frozen / repeat-install
    // fast paths.
    let update_requested = matches!(mode, EngineMode::Install) && options.update == Some(true);

    // `ignorePackageManifest` maps to pacquet's `ignore_manifest_check`: the
    // per-importer `package.json` ↔ `pnpm-lock.yaml` freshness gate is skipped,
    // so the install proceeds from the lockfile even when the in-memory
    // manifest disagrees with it. pnpm additionally skips the project-level
    // linking phase (its `pnpm fetch` semantics); pacquet still links direct
    // dependencies. The difference is immaterial to the programmatic consumers
    // that pass this option — a fuller native port is tracked in NAPI.md.
    let ignore_manifest_check = options.ignore_package_manifest == Some(true);

    // `enableModulesDir: false` ("do not create a `node_modules` directory") is
    // honored via pacquet's lockfile-only path: the graph resolves and the
    // lockfile is written, but nothing is materialized under `node_modules`.
    // Confined to the install path — a rebuild runs against an
    // already-materialized `node_modules`, so it must never take the
    // lockfile-only short-circuit (which would make it silently do nothing) even
    // when the caller reuses install options that disable the modules dir.
    let lockfile_only = matches!(mode, EngineMode::Install)
        && (options.lockfile_only.unwrap_or(false) || options.enable_modules_dir == Some(false));

    // A rebuild takes the frozen path against the already-materialized
    // `node_modules`, and re-runs dependency build scripts rather than the
    // root project's own lifecycle scripts.
    let frozen_lockfile = match &mode {
        EngineMode::Install => !update_requested && options.frozen_lockfile.unwrap_or(false),
        EngineMode::Rebuild(_) => true,
        // Peer issues need a full fresh resolve — never frozen.
        EngineMode::PeerIssues(_) => false,
    };
    let prefer_frozen_lockfile = if update_requested || matches!(mode, EngineMode::PeerIssues(_)) {
        Some(false)
    } else {
        options.prefer_frozen_lockfile
    };
    let update_seed_policy =
        if update_requested { UpdateSeedPolicy::DropAll } else { UpdateSeedPolicy::KeepAll };
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
                emit_initial_manifest: true,
                lockfile: MaybeLazyLockfile::Lazy(&lazy_lockfile),
                lockfile_path: lockfile_path.as_deref(),
                dependency_groups: groups,
                frozen_lockfile,
                prefer_frozen_lockfile,
                ignore_manifest_check,
                skip_runtimes: false,
                trust_lockfile: config.trust_lockfile,
                update_checksums: false,
                is_full_install,
                installs_only: true,
                supported_architectures: None,
                node_linker: config.node_linker,
                lockfile_only,
                // A peer-issue query resolves without writing anything;
                // the sink presence suppresses the CLI dry-run report.
                dry_run: matches!(mode, EngineMode::PeerIssues(_)),
                update_seed_policy,
                auth_override: None,
                resolution_observer: None,
                peer_issues_sink: match &mode {
                    EngineMode::PeerIssues(sink) => Some(Arc::clone(sink)),
                    EngineMode::Install | EngineMode::Rebuild(_) => None,
                },
                catalogs_override: None,
                // The optimistic repeat-install fast path skips
                // resolution entirely — a peer-issue query must never
                // short-circuit that way.
                disable_optimistic_repeat_install: matches!(mode, EngineMode::PeerIssues(_)),
                pnpmfile_hook_override: pnpmfile_hook,
                workspace_projects_override,
            };
            match mode {
                EngineMode::Install | EngineMode::PeerIssues(_) => {
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

fn build_overlay(options: &InstallOptions) -> napi::Result<ConfigOverlay> {
    let network_config = options.network_config.as_ref();
    Ok(ConfigOverlay {
        store_dir: options.store_dir.as_ref().map(PathBuf::from),
        cache_dir: options.cache_dir.as_ref().map(PathBuf::from),
        registry: None,
        registries: options.registries.as_ref().map(|map| map.clone().into_iter().collect()),
        proxy: options.proxy_config.as_ref().map(build_proxy_config).transpose()?,
        tls: network_config.map(build_tls_config).transpose()?,
        node_linker: options.node_linker.as_deref().and_then(parse_node_linker),
        link_workspace_packages: parse_link_workspace_packages(
            options.link_workspace_packages.as_ref(),
        )?,
        package_import_method: options
            .package_import_method
            .as_deref()
            .and_then(parse_import_method),
        virtual_store_dir_max_length: options.virtual_store_dir_max_length.map(u64::from),
        hoist_pattern: options.hoist_pattern.clone(),
        public_hoist_pattern: options.public_hoist_pattern.clone(),
        external_dependencies: options
            .external_dependencies
            .as_ref()
            .map(|items| items.iter().cloned().collect::<BTreeSet<_>>()),
        overrides: options.overrides.as_ref().map(|map| map.clone().into_iter().collect()),
        auto_install_peers: options.auto_install_peers,
        exclude_links_from_lockfile: options.exclude_links_from_lockfile,
        hoist_workspace_packages: options.hoist_workspace_packages,
        inject_workspace_packages: options.inject_workspace_packages,
        prefer_offline: options.prefer_offline,
        offline: options.offline,
        // `enableModulesDir: false` writes the lockfile while skipping
        // `node_modules`, so it runs through the lockfile-only path — which
        // requires the lockfile to be enabled. Force it on for that case so an
        // ambient `lockfile: false` can't turn the install into an opaque
        // `ERR_PNPM_CONFIG_CONFLICT_LOCKFILE_ONLY_WITH_NO_LOCKFILE`.
        lockfile: (options.enable_modules_dir == Some(false)).then_some(true),
        prefer_frozen_lockfile: options.prefer_frozen_lockfile,
        dedupe_peer_dependents: options.dedupe_peer_dependents,
        dedupe_direct_deps: options.dedupe_direct_deps,
        dedupe_injected_deps: options.dedupe_injected_deps,
        resolve_peers_from_workspace_root: options.resolve_peers_from_workspace_root,
        peers_suffix_max_length: options.peers_suffix_max_length.map(u64::from),
        network_concurrency: options
            .network_concurrency
            .or_else(|| network_config.and_then(|config| config.network_concurrency))
            .map(|value| value as usize),
        max_sockets: network_config
            .and_then(|config| config.max_sockets)
            .map(|value| value as usize),
        fetch_retries: options
            .fetch_retries
            .or_else(|| network_config.and_then(|config| config.fetch_retries)),
        fetch_retry_factor: options
            .fetch_retry_factor
            .or_else(|| network_config.and_then(|config| config.fetch_retry_factor)),
        fetch_retry_mintimeout: options
            .fetch_retry_mintimeout
            .or_else(|| network_config.and_then(|config| config.fetch_retry_mintimeout))
            .map(u64::from),
        fetch_retry_maxtimeout: options
            .fetch_retry_maxtimeout
            .or_else(|| network_config.and_then(|config| config.fetch_retry_maxtimeout))
            .map(u64::from),
        fetch_timeout: options
            .fetch_timeout
            .or_else(|| network_config.and_then(|config| config.fetch_timeout))
            .map(u64::from),
        user_agent: options
            .user_agent
            .clone()
            .or_else(|| network_config.and_then(|config| config.user_agent.clone())),
        // Embedders gate builds themselves, so default to report-not-fail.
        strict_dep_builds: Some(options.strict_dep_builds.unwrap_or(false)),
        allow_builds: options.allow_builds.clone().map(|map| map.into_iter().collect()),
        dangerously_allow_all_builds: options.dangerously_allow_all_builds,
        ignore_scripts: options.ignore_scripts,
        engine_strict: options.engine_strict,
        node_version: options.node_version.clone(),
        minimum_release_age: options.minimum_release_age.map(u64::from),
        minimum_release_age_exclude: options.minimum_release_age_exclude.clone(),
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
        auth_header_by_uri: options.auth_header_by_uri.clone().map(|map| map.into_iter().collect()),
    })
}

/// Reject a project whose `manifest` is not a JSON object up front.
/// `PackageManifest::from_value` coerces a non-object to `{}` as a last-resort
/// panic guard, but a silently-emptied manifest would drive resolution and
/// lockfile writing off missing data — so fail closed with a clear error here.
fn reject_non_object_manifests(projects: &[NodeApiProject]) -> napi::Result<()> {
    for project in projects {
        if !project.manifest.is_object() {
            return Err(invalid_manifest_error(&project.root_dir));
        }
    }
    Ok(())
}

fn reject_unsupported_install_options(options: &InstallOptions) -> napi::Result<()> {
    reject_non_empty_map(options.auth_config.as_ref(), "authConfig")?;
    reject_non_empty_list(options.never_built_dependencies.as_deref(), "neverBuiltDependencies")?;
    Ok(())
}

fn reject_non_empty_map<Value>(
    value: Option<&HashMap<String, Value>>,
    option: &str,
) -> napi::Result<()> {
    reject_if(value.is_some_and(|map| !map.is_empty()), option)
}

fn reject_non_empty_list<Value>(value: Option<&[Value]>, option: &str) -> napi::Result<()> {
    reject_if(value.is_some_and(|items| !items.is_empty()), option)
}

fn reject_if(condition: bool, option: &str) -> napi::Result<()> {
    if condition { Err(unsupported_option_error("install", option)) } else { Ok(()) }
}

fn build_proxy_config(input: &ProxyConfigInput) -> napi::Result<ProxyConfig> {
    Ok(ProxyConfig {
        https_proxy: input.https_proxy.clone(),
        http_proxy: input.http_proxy.clone(),
        no_proxy: input.no_proxy.as_ref().map(parse_no_proxy).transpose()?.flatten(),
    })
}

fn parse_no_proxy(value: &serde_json::Value) -> napi::Result<Option<NoProxySetting>> {
    match value {
        serde_json::Value::Bool(true) => Ok(Some(NoProxySetting::Bypass)),
        serde_json::Value::Bool(false) | serde_json::Value::Null => Ok(None),
        serde_json::Value::String(items) => Ok(Some(NoProxySetting::List(
            items
                .split(',')
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .map(ToOwned::to_owned)
                .collect(),
        ))),
        _ => Err(unsupported_option_error("install", "proxyConfig.noProxy")),
    }
}

fn build_tls_config(input: &NetworkConfigInput) -> napi::Result<TlsConfig> {
    Ok(TlsConfig {
        ca: input.ca.as_ref().map(parse_string_list).transpose()?.unwrap_or_default(),
        cert: input.cert.as_ref().map(parse_single_string).transpose()?.flatten(),
        key: input.key.clone(),
        strict_ssl: input.strict_ssl,
        local_address: input
            .local_address
            .as_deref()
            .and_then(|value| value.parse::<IpAddr>().ok()),
    })
}

fn parse_string_list(value: &serde_json::Value) -> napi::Result<Vec<String>> {
    match value {
        serde_json::Value::String(item) => Ok(vec![item.clone()]),
        serde_json::Value::Array(items) => items
            .iter()
            .map(|item| match item {
                serde_json::Value::String(value) => Ok(value.clone()),
                _ => Err(unsupported_option_error("install", "networkConfig.ca")),
            })
            .collect(),
        _ => Err(unsupported_option_error("install", "networkConfig.ca")),
    }
}

fn parse_single_string(value: &serde_json::Value) -> napi::Result<Option<String>> {
    match value {
        serde_json::Value::Null => Ok(None),
        serde_json::Value::String(item) => Ok(Some(item.clone())),
        _ => Err(unsupported_option_error("install", "networkConfig.cert")),
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

/// Parse the JS `linkWorkspacePackages` value (`true` / `false` / `"deep"`)
/// into a [`pacquet_config::LinkWorkspacePackages`], reusing the config
/// crate's `Deserialize`. Rejects any other value.
fn parse_link_workspace_packages(
    value: Option<&serde_json::Value>,
) -> napi::Result<Option<pacquet_config::LinkWorkspacePackages>> {
    value
        .map(|value| {
            serde_json::from_value::<pacquet_config::LinkWorkspacePackages>(value.clone()).map_err(
                |error| {
                    napi::Error::from_reason(format!(
                        r#"invalid linkWorkspacePackages (expected true, false, or "deep"): {error}"#,
                    ))
                },
            )
        })
        .transpose()
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
    let _guard = engine_call_lock().lock().await;
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
    // Restores the previous sink on drop — including on a panic in
    // `run_install_inner`, which unwinds this dedicated thread.
    let _sink_guard = EngineCallGuard::new(on_log);
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

#[napi(js_name = "getPeerDependencyIssues")]
pub async fn get_peer_dependency_issues(
    options: serde_json::Value,
) -> napi::Result<serde_json::Value> {
    let _guard = engine_call_lock().lock().await;
    let (tx, rx) = tokio::sync::oneshot::channel();
    std::thread::Builder::new()
        .name("pnpm-napi-peer-issues".to_string())
        .stack_size(32 * 1024 * 1024)
        .spawn(move || {
            let _ = tx.send(run_peer_issues_blocking(&options));
        })
        .map_err(|error| {
            napi::Error::from_reason(format!("failed to spawn peer-issues thread: {error}"))
        })?;
    rx.await.map_err(|_| napi::Error::from_reason("peer-issues worker thread panicked"))?
}

/// Map the `PeerIssuesOptions` JSON (a subset of [`InstallOptions`] — see
/// `index.d.ts`) onto the install options struct, run a sink-driven
/// `dry_run` resolve, and serialize the per-importer issues into the
/// `PeerDependencyIssuesByProjects` wire shape, including the
/// `conflicts` / `intersections` derivation v11's `mergePeers` does.
fn run_peer_issues_blocking(options: &serde_json::Value) -> napi::Result<serde_json::Value> {
    // No log sink for this query — engine events would interleave with
    // the caller's own reporting for what is a silent resolution.
    let _sink_guard = EngineCallGuard::new(None);
    let obj = options.as_object().ok_or_else(|| {
        napi::Error::from_reason("getPeerDependencyIssues: options must be an object")
    })?;
    let str_field =
        |key: &str| obj.get(key).and_then(serde_json::Value::as_str).map(ToString::to_string);
    let string_map = |key: &str| {
        obj.get(key).and_then(serde_json::Value::as_object).map(|map| {
            map.iter()
                .filter_map(|(key, value)| {
                    value.as_str().map(|value| (key.clone(), value.to_string()))
                })
                .collect::<HashMap<String, String>>()
        })
    };
    let dir = str_field("dir")
        .ok_or_else(|| napi::Error::from_reason("getPeerDependencyIssues: `dir` is required"))?;
    let projects: Vec<NodeApiProject> = obj
        .get("projects")
        .and_then(serde_json::Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .filter_map(|entry| {
                    let entry = entry.as_object()?;
                    Some(NodeApiProject {
                        root_dir: entry.get("rootDir")?.as_str()?.to_string(),
                        manifest: entry
                            .get("manifest")
                            .cloned()
                            .unwrap_or_else(|| serde_json::json!({})),
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    let install_options = InstallOptions {
        dir,
        projects,
        store_dir: str_field("storeDir"),
        cache_dir: str_field("cacheDir"),
        registries: string_map("registries"),
        auth_header_by_uri: string_map("authHeaderByUri"),
        overrides: string_map("overrides"),
        // Report every missing peer: with pnpm's default
        // `autoInstallPeers: true` the resolver satisfies the peer
        // itself and the issue never surfaces, but this query's whole
        // point is the report (Bit derives the `intersections` it
        // auto-adds from it). Callers can still opt back in.
        auto_install_peers: Some(
            obj.get("autoInstallPeers").and_then(serde_json::Value::as_bool).unwrap_or(false),
        ),
        peers_suffix_max_length: obj
            .get("peersSuffixMaxLength")
            .and_then(serde_json::Value::as_u64)
            .map(|value| value as u32),
        virtual_store_dir_max_length: obj
            .get("virtualStoreDirMaxLength")
            .and_then(serde_json::Value::as_u64)
            .map(|value| value as u32),
        ..InstallOptions::default()
    };

    let sink: pacquet_package_manager::PeerIssuesSink = Arc::default();
    run_install_inner(&install_options, None, EngineMode::PeerIssues(Arc::clone(&sink)))?;
    let issues_by_importer =
        std::mem::take(&mut *sink.lock().expect("peer-issues sink lock poisoned"));

    let mut result = serde_json::Map::new();
    for (importer_id, issues) in issues_by_importer {
        result.insert(importer_id, peer_issues_to_json(&issues));
    }
    Ok(serde_json::Value::Object(result))
}

/// Serialize one importer's issues into v11's `PeerDependencyIssues`
/// wire shape, deriving `conflicts` / `intersections` from the missing
/// peers the way v11's `mergePeers` does: all-optional names are
/// skipped, a single range passes through verbatim, and multiple
/// ranges intersect via semver bound-set intersection (`null`
/// intersection → conflict).
fn peer_issues_to_json(
    issues: &pacquet_resolving_deps_resolver::PeerDependencyIssues,
) -> serde_json::Value {
    let parents_json = |parents: &[pacquet_resolving_deps_resolver::ParentPackageRef]| {
        parents
            .iter()
            .map(|parent| serde_json::json!({ "name": parent.name, "version": parent.version }))
            .collect::<Vec<_>>()
    };

    let mut missing = serde_json::Map::new();
    for (peer_name, entries) in &issues.missing {
        missing.insert(
            peer_name.clone(),
            entries
                .iter()
                .map(|entry| {
                    serde_json::json!({
                        "parents": parents_json(&entry.parents),
                        "optional": entry.optional,
                        "wantedRange": entry.wanted_range,
                    })
                })
                .collect(),
        );
    }

    let mut bad = serde_json::Map::new();
    for (peer_name, entries) in &issues.bad {
        bad.insert(
            peer_name.clone(),
            entries
                .iter()
                .map(|entry| {
                    serde_json::json!({
                        "parents": parents_json(&entry.parents),
                        "foundVersion": entry.found_version,
                        "resolvedFrom": parents_json(&entry.resolved_from),
                        "optional": entry.optional,
                        "wantedRange": entry.wanted_range,
                    })
                })
                .collect(),
        );
    }

    let mut conflicts: Vec<String> = Vec::new();
    let mut intersections = serde_json::Map::new();
    for (peer_name, entries) in &issues.missing {
        if entries.iter().all(|entry| entry.optional) {
            continue;
        }
        if let [entry] = entries.as_slice() {
            intersections
                .insert(peer_name.clone(), serde_json::Value::String(entry.wanted_range.clone()));
            continue;
        }
        match safe_intersect(entries.iter().map(|entry| entry.wanted_range.as_str())) {
            Some(intersection) => {
                intersections.insert(peer_name.clone(), serde_json::Value::String(intersection));
            }
            None => conflicts.push(peer_name.clone()),
        }
    }
    conflicts.sort_unstable();

    serde_json::json!({
        "missing": missing,
        "bad": bad,
        "conflicts": conflicts,
        "intersections": intersections,
    })
}

/// Intersect semver ranges pairwise. `None` when any range fails to
/// parse or the intersection is empty — the caller records a conflict,
/// matching v11's `safeIntersect` (which swallows
/// `semver-range-intersect` errors the same way).
fn safe_intersect<'a>(ranges: impl Iterator<Item = &'a str>) -> Option<String> {
    let mut acc: Option<node_semver::Range> = None;
    for range in ranges {
        let parsed: node_semver::Range = range.parse().ok()?;
        acc = Some(match acc {
            None => parsed,
            Some(current) => current.intersect(&parsed)?,
        });
    }
    acc.map(|range| range.to_string())
}

#[cfg(test)]
mod tests;
