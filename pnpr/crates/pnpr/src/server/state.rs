use super::{
    AuthState, CanonicalPackageName, Config, LoginRateLimiter, RegistryError, ServerMetrics,
    Singleflight, Storage, StripedLocks,
    package_responses::processing::PackumentProcessing,
    publishing::retry::ReplayStorage,
    registry_graph::{RoutingSnapshot, UpstreamGuardContext, build_snapshot},
};
use pnpr_config::{Management, RoutingConfig};
use pnpr_route::RouteContext;
use std::sync::{Arc, RwLock};

#[cfg_attr(
    dylint_lib = "perfectionist",
    expect(
        perfectionist::too_many_struct_fields,
        reason = "server inner state groups core subsystems"
    )
)]
pub(crate) struct AppInner {
    pub(super) storage: Storage,
    /// The server configuration. Its `routing` holds only
    /// `routes.allowedPrivateNetworks`: the registry graph lives in
    /// [`Self::routing`].
    pub(crate) config: Config,
    /// The registry graph requests route by. Read it through
    /// [`super::AppState::routing`].
    pub(super) routing: RwLock<Arc<RoutingSnapshot>>,
    /// How many snapshots [`Self::routing`] has replaced. See
    /// [`RoutingSnapshot::epoch`].
    pub(super) routing_epochs: std::sync::atomic::AtomicU64,
    pub(super) upstream_guard_context: UpstreamGuardContext,
    /// The resolver state every routing snapshot's engine shares. Built on the
    /// first resolve, so servers that never receive one pay nothing.
    pub(super) resolver_shared: std::sync::OnceLock<crate::resolver::ResolverShared>,
    /// Local OSV index, loaded before the server accepts requests when
    /// `osv.enabled` is set and a mounted surface consults it.
    pub(super) osv_index: Option<Arc<pnpr_osv::OsvIndex>>,
    pub(super) builds: BuildServices,
    pub(super) proxy: ProxyState,
    pub(super) packuments: PackumentProcessing,
    pub(super) locks: MutationLocks,
    pub(super) identity: IdentityServices,
    pub(crate) metrics: Arc<ServerMetrics>,
    pub(super) shutting_down: std::sync::atomic::AtomicBool,
}

type PackumentSingleflight = Singleflight<
    (String, CanonicalPackageName),
    Result<Option<Arc<super::upstream_packuments::AdmittedPackument>>, Arc<RegistryError>>,
>;
pub(super) struct BuildServices {
    pub(super) artifacts: Option<pnpr_shared_artifacts::SharedArtifactStore>,
    pub(super) compiler_cache_uploads: tokio::sync::Semaphore,
    pub(super) pipeline_runs: Option<pnpr_pipeline_runs::PipelineRunStore>,
}

pub(super) struct ProxyState {
    pub(super) packument_singleflight: PackumentSingleflight,
    pub(super) tar_variants: super::TarVariantQueue,
    /// Permits for tar responses and publishes that decode a `.tgz`. See
    /// [`super::MAX_CONCURRENT_TAR_DECODES`].
    pub(super) tar_decodes: Arc<tokio::sync::Semaphore>,
    pub(super) background_tar_scans: Arc<super::BackgroundTarScans>,
    pub(super) integrity_backfill: super::upstream_integrity::BackgroundIntegrity,
    pub(super) version_tgzs: super::VersionTgzCache,
}

pub(super) struct MutationLocks {
    /// Serializes the read-modify-write packument flows per package so
    /// two concurrent writers to the same package on this instance can't
    /// lose each other's changes.
    pub(super) packages: StripedLocks,
    pub(super) referrer_migrations: StripedLocks,
    pub(super) publish_concurrency: tokio::sync::Semaphore,
    pub(super) publish_replays: ReplayStorage,
    /// Release archives being received for staging, each buffered whole. See
    /// [`super::staged_archives::MAX_CONCURRENT_STAGED_UPLOADS`].
    pub(super) staged_archive_uploads: tokio::sync::Semaphore,
    /// The same, per principal, so one account cannot hold every upload
    /// permit.
    pub(super) staged_archive_uploads_by_principal: super::KeyedSemaphores,
    pub(super) logs: super::release_record_logs::LogIntegrationState,
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
        mut config: Config,
        auth: AuthState,
        osv_index: Option<Arc<pnpr_osv::OsvIndex>>,
    ) -> pnpr_error::Result<Self> {
        let storage = open_storage(&config)?;
        let builds = BuildServices::new(&config, &storage)?;
        let routing = std::mem::take(&mut config.routing);
        config.routing.route_policy.allowed_private_networks.clone_from(
            &routing.route_policy.allowed_private_networks,
        );
        let identity = IdentityServices::new(&config, &routing, auth)?;
        let publish_replays = ReplayStorage::new(&config.storage.cache_dir);
        let upstream_guard_context: UpstreamGuardContext = Arc::new(RwLock::new(Arc::new(
            RouteContext::from_routing(&routing, &config.http.public_url),
        )));
        let snapshot = build_snapshot(&config, routing, None, &upstream_guard_context);
        Ok(Self {
            storage,
            config,
            routing: RwLock::new(Arc::new(snapshot)),
            routing_epochs: std::sync::atomic::AtomicU64::new(0),
            upstream_guard_context,
            resolver_shared: std::sync::OnceLock::new(),
            osv_index,
            builds,
            proxy: ProxyState::new(),
            packuments: PackumentProcessing::default(),
            locks: MutationLocks {
                packages: StripedLocks::new(),
                referrer_migrations: StripedLocks::new(),
                publish_concurrency: tokio::sync::Semaphore::new(16),
                publish_replays,
                staged_archive_uploads: tokio::sync::Semaphore::new(
                    super::staged_archives::MAX_CONCURRENT_STAGED_UPLOADS,
                ),
                staged_archive_uploads_by_principal: super::KeyedSemaphores::new(
                    super::staged_archives::MAX_CONCURRENT_STAGED_UPLOADS_PER_PRINCIPAL,
                ),
                logs: super::release_record_logs::LogIntegrationState::default(),
            },
            identity,
            metrics: Arc::new(ServerMetrics::new()),
            shutting_down: std::sync::atomic::AtomicBool::new(false),
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
    fn new() -> Self {
        Self {
            packument_singleflight: Singleflight::new(),
            tar_variants: super::TarVariantQueue::new(),
            tar_decodes: Arc::new(tokio::sync::Semaphore::new(super::MAX_CONCURRENT_TAR_DECODES)),
            background_tar_scans: super::BackgroundTarScans::new(),
            integrity_backfill: super::upstream_integrity::BackgroundIntegrity::default(),
            version_tgzs: super::VersionTgzCache::default(),
        }
    }
}

impl IdentityServices {
    fn new(config: &Config, routing: &RoutingConfig, auth: AuthState) -> pnpr_error::Result<Self> {
        super::scim::validate_oidc(config)?;
        // A stored graph is checked against these when it loads and when it
        // changes; see `registry_graph::check_file_references`.
        if config.registries_managed_by == Management::Config {
            super::oidc::validate_workloads(config, routing)?;
            super::oidc_groups::validate_group_grants(config, routing)?;
        }
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

fn open_storage(config: &Config) -> pnpr_error::Result<Storage> {
    let storage = Storage::from_config(&config.storage)?;
    if config.registries_managed_by == Management::Api {
        storage.refuse_publishes_into_retired_namespaces();
    }
    Ok(storage)
}
