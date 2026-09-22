use super::{
    AuthState,
    Config,
    Storage,
    StripedLocks,
    Upstream,
    compute_upstream_cache_namespace,
};
use indexmap::IndexMap;
use pnpr_registry::Ecosystem;
use std::sync::Arc;

pub(super) struct AppInner {
    pub(super) storage: Storage,
    pub(super) config: Config,
    /// Lazily-built engine backing the `/-/pnpr/v0/resolve` endpoint. Built on
    /// first such request so servers that never receive one pay nothing.
    pub(super) resolver: std::sync::OnceLock<crate::resolver::Resolver>,
    /// Local OSV index, loaded before the server accepts requests when
    /// `osv.enabled` is set and a mounted surface consults it.
    pub(super) osv_index: Option<Arc<pnpr_osv::OsvIndex>>,
    pub(super) builds: BuildServices,
    pub(super) proxy: ProxyState,
    pub(super) locks: MutationLocks,
    pub(super) identity: IdentityServices,
}
pub(super) struct BuildServices {
    pub(super) artifacts: Option<pnpr_shared_artifacts::SharedArtifactStore>,
    pub(super) compiler_cache_uploads: tokio::sync::Semaphore,
    pub(super) pipeline_runs: Option<pnpr_pipeline_runs::PipelineRunStore>,
}

pub(super) struct ProxyState {
    /// One [`Upstream`] per declared upstream, keyed by the same name
    /// used in [`pnpr_config::RoutingConfig::upstreams`]. Built once at router construction
    /// time so each request avoids re-allocating a `ThrottledClient`.
    pub(super) upstreams: IndexMap<String, Upstream>,
    /// The disposable cache namespace of each upstream, keyed like
    /// [`Self::upstreams`]. A pure function of the config (see
    /// [`compute_upstream_cache_namespace`]), precomputed here so the
    /// per-request path doesn't re-sort and re-hash the upstream's headers on
    /// every packument and tarball served through an upstream registry.
    pub(super) cache_namespaces: IndexMap<String, String>,
}

pub(super) struct MutationLocks {
    /// Serializes the read-modify-write packument flows per package so
    /// two concurrent writers to the same package on this instance can't
    /// lose each other's changes.
    pub(super) packages: StripedLocks,
    pub(super) referrer_migrations: StripedLocks,
}

pub(super) struct IdentityServices {
    pub(super) auth: AuthState,
    pub(super) oidc: pnpr_auth::oidc::OidcState,
}

impl AppInner {
    pub(super) fn new(
        config: Config,
        auth: AuthState,
        osv_index: Option<Arc<pnpr_osv::OsvIndex>>,
    ) -> pnpr_error::Result<Self> {
        let storage = Storage::new(
            &config.storage.hosted_backend,
            config.storage.hosted_dir.clone(),
            config.storage.cache_dir.clone(),
        )?;
        let builds = BuildServices::new(&config, &storage)?;
        let proxy = ProxyState::new(&config);
        let identity = IdentityServices::new(&config, auth)?;
        Ok(Self {
            storage,
            config,
            resolver: std::sync::OnceLock::new(),
            osv_index,
            builds,
            proxy,
            locks: MutationLocks {
                packages: StripedLocks::new(),
                referrer_migrations: StripedLocks::new(),
            },
            identity,
        })
    }
}

impl BuildServices {
    fn new(config: &Config, storage: &Storage) -> pnpr_error::Result<Self> {
        Ok(Self {
            artifacts: artifact_store(config, config.features.artifacts.enabled)?,
            compiler_cache_uploads: tokio::sync::Semaphore::new(2),
            pipeline_runs: config.features.pipeline.enabled.then(|| {
                pnpr_pipeline_runs::PipelineRunStore::new(storage.clone())
            }),
        })
    }
}

impl ProxyState {
    fn new(config: &Config) -> Self {
        let upstreams = upstream_clients(config, config.features.registry.enabled);
        let upstream_cache_namespaces = config.routing.upstreams
            .keys()
            .map(|name| (name.clone(), compute_upstream_cache_namespace(config, name)))
            .collect();
        Self { upstreams, cache_namespaces: upstream_cache_namespaces }
    }
}

impl IdentityServices {
    fn new(config: &Config, auth: AuthState) -> pnpr_error::Result<Self> {
        super::oidc::validate_workloads(config)?;
        let oidc =
            pnpr_auth::oidc::OidcState::new(&config.identity.auth.oidc, &config.http.public_url)?;
        Ok(Self { auth, oidc })
    }
}

/// Only the registry routes consult the upstreams, so a resolver-only
/// server builds none, skipping a `ThrottledClient` allocation per
/// configured upstream.
fn upstream_clients(config: &Config, registry_enabled: bool) -> IndexMap<String, Upstream> {
    if !registry_enabled {
        return IndexMap::new();
    }
    config.routing.upstreams
        .iter()
        .map(|(name, upstream)| {
            let client = Upstream::new(name, upstream);
            let client = if config.routing.registries.ecosystem(name) == Some(Ecosystem::Npm) {
                client
            } else {
                client.with_fetch_guard(super::ecosystem::upstream_fetch_guard(config, upstream))
            };
            (name.clone(), client)
        })
        .collect()
}

/// The compression and access-log layers every response passes through.
fn artifact_store(
    config: &Config,
    enabled: bool,
) -> pnpr_error::Result<Option<pnpr_shared_artifacts::SharedArtifactStore>> {
    enabled
        .then(|| {
            pnpr_shared_artifacts::SharedArtifactStore::new(
                &config.storage.hosted_backend,
                &config.storage.cache_dir,
            )
        })
        .transpose()
}
