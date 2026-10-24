use super::{
    AuthState, CanonicalPackageName, Config, LoginRateLimiter, RegistryError, ServerMetrics,
    Singleflight, Storage, StripedLocks, Upstream, compute_upstream_cache_namespace,
};
use indexmap::IndexMap;
use std::sync::Arc;

#[cfg_attr(
    dylint_lib = "perfectionist",
    expect(
        perfectionist::too_many_struct_fields,
        reason = "server inner state groups core subsystems"
    )
)]
pub(crate) struct AppInner {
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
    pub(crate) metrics: Arc<ServerMetrics>,
}

type PackumentSingleflight =
    Singleflight<(String, CanonicalPackageName), Result<Option<Vec<u8>>, Arc<RegistryError>>>;
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
    pub(super) packument_singleflight: PackumentSingleflight,
    pub(super) tar_variants: super::TarVariantQueue,
    /// Permits for tar responses and publishes that decode a `.tgz`. See
    /// [`super::MAX_CONCURRENT_TAR_DECODES`].
    pub(super) tar_decodes: Arc<tokio::sync::Semaphore>,
    pub(super) background_tar_scans: Arc<super::BackgroundTarScans>,
    pub(super) version_tgzs: super::VersionTgzCache,
}

pub(super) struct MutationLocks {
    /// Serializes the read-modify-write packument flows per package so
    /// two concurrent writers to the same package on this instance can't
    /// lose each other's changes.
    pub(super) packages: StripedLocks,
    pub(super) referrer_migrations: StripedLocks,
    pub(super) publish_concurrency: tokio::sync::Semaphore,
}

pub(super) struct IdentityServices {
    pub(super) auth: AuthState,
    pub(super) last_used: pnpr_auth::LastUsedWriter,
    pub(super) oidc: pnpr_auth::oidc::OidcState,
    pub(super) managed: super::managed_state::ManagedState,
    pub(super) login_rate_limiter: LoginRateLimiter,
    pub(super) login_concurrency: tokio::sync::Semaphore,
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
                publish_concurrency: tokio::sync::Semaphore::new(16),
            },
            identity,
            metrics: Arc::new(ServerMetrics::new()),
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
        Self {
            upstreams,
            cache_namespaces: upstream_cache_namespaces,
            packument_singleflight: Singleflight::new(),
            tar_variants: super::TarVariantQueue::new(),
            tar_decodes: Arc::new(tokio::sync::Semaphore::new(super::MAX_CONCURRENT_TAR_DECODES)),
            background_tar_scans: super::BackgroundTarScans::new(),
            version_tgzs: super::VersionTgzCache::default(),
        }
    }
}

impl IdentityServices {
    fn new(config: &Config, auth: AuthState) -> pnpr_error::Result<Self> {
        super::oidc::validate_workloads(config)?;
        super::oidc_groups::validate_group_grants(config)?;
        let oidc =
            pnpr_auth::oidc::OidcState::new(&config.identity.auth.oidc, &config.http.public_url)?;
        Ok(Self {
            last_used: pnpr_auth::LastUsedWriter::new(Arc::clone(&auth.tokens)),
            auth,
            oidc,
            managed: super::managed_state::ManagedState::new(config),
            login_rate_limiter: LoginRateLimiter::default(),
            login_concurrency: tokio::sync::Semaphore::new(8),
        })
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
            let client = Upstream::new(name, upstream)
                .with_fetch_guard(super::ecosystem::upstream_fetch_guard(config, upstream));
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
            .map(|store| store.with_quota(config.features.artifacts.quota))
        })
        .transpose()
}
