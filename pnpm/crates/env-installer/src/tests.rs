use crate::{
    ConfigDepError, ConfigDepsInstallOptions, install_config_deps, is_package_manager_resolved,
    pnpm_engine_packages, prune_env_lockfile, resolve_and_install_config_deps,
    resolve_package_manager_integrities,
};
use pnpm_lockfile::{
    EnvLockfile, LockfileResolution, PackageKey, PackageMetadata, RegistryResolution,
    SnapshotDepRef, SnapshotEntry, SpecifierAndResolution, TarballResolution,
};
use pnpm_network::{AuthHeaders, RetryOpts, ThrottledClient};
use pnpm_reporter::{InstallingConfigDepsStatus, LogEvent, Reporter, SilentReporter};
use pnpm_resolving_npm_resolver::{
    InMemoryPackageMetaCache, NpmResolver, shared_packument_fetch_locker,
    shared_picked_manifest_cache,
};
use pnpm_resolving_resolver_base::{
    LatestInfo, LatestQuery, PkgResolutionId, ResolveFuture, ResolveLatestFuture, ResolveOptions,
    ResolveResult, Resolver, WantedDependency,
};
use pnpm_store_dir::StoreDir;
use pnpm_testing_utils::registry::TestRegistry;
use pnpm_workspace_state::ConfigDependency;
use std::{
    collections::{BTreeMap, HashMap},
    path::Path,
    sync::{Arc, Mutex},
};
use tempfile::TempDir;

/// Resolve `name@version` against the mock registry and return its
/// integrity string, so migration tests can build the old inline
/// `<version>+<integrity>` format without hard-coding a checksum.
async fn integrity_of(
    resolver: &NpmResolver<InMemoryPackageMetaCache>,
    name: &str,
    version: &str,
) -> String {
    let wanted = WantedDependency {
        alias: Some(name.to_string()),
        bare_specifier: Some(version.to_string()),
        ..WantedDependency::default()
    };
    let result = resolver.resolve(&wanted, &ResolveOptions::default()).await.unwrap().unwrap();
    match result.resolution {
        LockfileResolution::Tarball(tarball) => tarball.integrity.unwrap().to_string(),
        LockfileResolution::Registry(registry) => registry.integrity.to_string(),
        other => panic!("unexpected resolution: {other:?}"),
    }
}

/// Resolve `name@version` against the mock registry and return the tarball
/// URL its packument advertises, so a test can assert on the whole URL
/// without hard-coding the registry's path layout.
async fn tarball_url_of(
    resolver: &NpmResolver<InMemoryPackageMetaCache>,
    name: &str,
    version: &str,
) -> String {
    let wanted = WantedDependency {
        alias: Some(name.to_string()),
        bare_specifier: Some(version.to_string()),
        ..WantedDependency::default()
    };
    let result = resolver.resolve(&wanted, &ResolveOptions::default()).await.unwrap().unwrap();
    match result.resolution {
        LockfileResolution::Tarball(tarball) => tarball.tarball,
        other => panic!("unexpected resolution: {other:?}"),
    }
}

/// Build an npm resolver pointing at the in-process mock registry.
fn build_resolver(registry: &str) -> (NpmResolver<InMemoryPackageMetaCache>, TempDir) {
    let cache_dir = TempDir::new().unwrap();
    let mut registries = std::collections::HashMap::new();
    registries.insert("default".to_string(), registry.to_string());
    let resolver = NpmResolver {
        registries,
        registries_by_prefix: std::collections::HashMap::new(),
        http_client: Arc::new(ThrottledClient::default()),
        auth_headers: Arc::new(AuthHeaders::default()),
        meta_cache: Arc::new(InMemoryPackageMetaCache::default()),
        fetch_locker: shared_packument_fetch_locker(),
        picked_manifest_cache: shared_picked_manifest_cache(),
        cache_dir: Some(cache_dir.path().to_path_buf()),
        offline: false,
        prefer_offline: false,
        ignore_missing_time_field: false,
        full_metadata: false,
        needs_full_metadata_for: None,
        filter_metadata: false,
        retry_opts: RetryOpts::default(),
    };
    (resolver, cache_dir)
}

/// Per-test handles kept alive for the duration of an install call.
struct Harness {
    registry_url: String,
    registries: std::collections::HashMap<String, String>,
    http_client: ThrottledClient,
    auth_headers: AuthHeaders,
    store_dir: &'static StoreDir,
    /// Owns the directory `store_dir` points at. Only the [`StoreDir`] value
    /// has to be `'static`, so the directory itself is dropped with the
    /// harness rather than left behind for the run's temp dir to accumulate.
    _store_root: TempDir,
}

fn harness() -> Harness {
    let registry_url = TestRegistry::start().url();
    let mut registries = std::collections::HashMap::new();
    registries.insert("default".to_string(), registry_url.clone());
    let store_root = TempDir::new().unwrap();
    let store_dir: &'static StoreDir =
        Box::leak(Box::new(StoreDir::new(store_root.path().to_path_buf())));
    Harness {
        registry_url,
        registries,
        http_client: ThrottledClient::default(),
        auth_headers: AuthHeaders::default(),
        store_dir,
        _store_root: store_root,
    }
}

fn options<'a>(
    harness: &'a Harness,
    root_dir: &'a Path,
    frozen: bool,
) -> ConfigDepsInstallOptions<'a> {
    ConfigDepsInstallOptions {
        root_dir,
        store_dir: harness.store_dir,
        http_client: &harness.http_client,
        auth_headers: &harness.auth_headers,
        registries: &harness.registries,
        verify_store_integrity: true,
        strict_store_pkg_content_check: true,
        offline: false,
        package_import_method: pnpm_config::PackageImportMethod::default(),
        retry_opts: RetryOpts::default(),
        frozen_lockfile: frozen,
        supported_architectures: None,
        current_node_version: "20.0.0",
        current_os: "linux",
        current_cpu: "x64",
        current_libc: "glibc",
    }
}

fn clean_spec(version: &str) -> ConfigDependency {
    ConfigDependency::VersionWithIntegrity(version.to_string())
}

#[derive(Default)]
struct FixtureResolver {
    packages: HashMap<(String, String), serde_json::Value>,
    /// Resolve to tarball resolutions whose URL is not derivable from the
    /// registry — the shape a load-balanced proxy or Artifactory-style
    /// mirror produces.
    non_derivable_tarball_urls: bool,
}

impl FixtureResolver {
    fn new() -> Self {
        Self::default()
    }

    fn package(mut self, manifest: serde_json::Value) -> Self {
        let name = manifest["name"].as_str().expect("fixture package name").to_string();
        let version = manifest["version"].as_str().expect("fixture package version").to_string();
        self.packages.insert((name, version), manifest);
        self
    }

    fn with_non_derivable_tarball_urls(mut self) -> Self {
        self.non_derivable_tarball_urls = true;
        self
    }
}

impl Resolver for FixtureResolver {
    fn resolve<'a>(
        &'a self,
        wanted_dependency: &'a WantedDependency,
        _opts: &'a ResolveOptions,
    ) -> ResolveFuture<'a> {
        Box::pin(async move {
            let Some(alias) = wanted_dependency.alias.as_deref() else {
                return Ok(None);
            };
            let Some(specifier) = wanted_dependency.bare_specifier.as_deref() else {
                return Ok(None);
            };
            let Some(manifest) =
                self.packages.get(&(alias.to_string(), specifier.to_string())).cloned()
            else {
                return Ok(None);
            };
            let name = manifest["name"].as_str().expect("fixture package name").to_string();
            let version =
                manifest["version"].as_str().expect("fixture package version").to_string();
            let id = format!("{name}@{version}");
            Ok(Some(ResolveResult {
                id: PkgResolutionId::from(id.as_str()),
                name_ver: Some(id.parse().expect("fixture name/version parses")),
                latest: Some(version.clone()),
                published_at: None,
                manifest: Some(Arc::new(manifest)),
                resolution: if self.non_derivable_tarball_urls {
                    LockfileResolution::Tarball(TarballResolution {
                        tarball: format!(
                            "https://mirror-pool-7.example.com/registry/{name}/-/{name}-{version}.tgz",
                        ),
                        integrity: Some(ssri::Integrity::from(id.as_bytes())),
                        revision: None,
                        git_hosted: None,
                        path: None,
                    })
                } else {
                    LockfileResolution::Registry(RegistryResolution {
                        integrity: ssri::Integrity::from(id.as_bytes()),
                        revision: None,
                    })
                },
                resolved_via: "npm-registry".to_string(),
                normalized_bare_specifier: Some(specifier.to_string()),
                alias: Some(alias.to_string()),
                policy_violation: None,
            }))
        })
    }

    fn resolve_latest<'a>(
        &'a self,
        _query: &'a LatestQuery,
        _opts: &'a ResolveOptions,
    ) -> ResolveLatestFuture<'a> {
        Box::pin(async { Ok(Some(LatestInfo::default())) })
    }
}

/// Recursively search `dir` for an entry named `name`, without following
/// symlinks (so it can't loop through the dir links a successful install leaves).
fn contains_entry_named(dir: &Path, name: &str) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    for entry in entries.flatten() {
        if entry.file_name() == name {
            return true;
        }
        if entry.file_type().is_ok_and(|file_type| file_type.is_dir())
            && contains_entry_named(&entry.path(), name)
        {
            return true;
        }
    }
    false
}

mod configuration;

mod dependencies;

mod integrity;

mod behavior;

mod lockfile;

mod security;

mod streaming;
