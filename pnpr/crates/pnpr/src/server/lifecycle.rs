mod cache_claim;

use self::cache_claim::claim_ephemeral_cache_dir;
use std::future::IntoFuture;

use axum::Router;
use futures_util::FutureExt as _;

use super::{
    AppState, Arc, AuthState, Config, Ecosystem, NodelayTcpListener, PeerAddr, Storage,
    metrics_listener_router,
    registry_graph::{Credentials, FileReferences},
    shutdown_signal, state_and_router,
};
use pnpr_config::{GraphSource, RoutingConfig};

/// Load the OSV index only for surfaces that consult it. An artifacts-only
/// tier skips the database because artifact requests do not use OSV data.
pub(super) fn load_active_osv_index(
    config: &Config,
) -> pnpr_error::Result<Option<Arc<pnpr_osv::OsvIndex>>> {
    if config.features.resolver.enabled || config.features.registry.enabled {
        pnpr_osv::load_osv_index(config)
    } else {
        Ok(None)
    }
}

/// Check the hosted store's layout, empty a proxy cache in another layout,
/// and reconcile hosted transparency logs with the metadata database.
/// [`serve`] and [`serve_listener`] call this before binding; an embedder that
/// builds a router directly should call it itself on startup, before serving
/// requests.
///
/// A stored graph that cannot be loaded does not stop it: the store is bound
/// all the same, and only the log reconcile, which walks the hosted
/// registries, is skipped.
pub async fn prepare_storage(config: &Config) -> pnpr_error::Result<()> {
    crate::config_validation::validate_cache_secret_for_claim(config)?;
    let storage = Storage::from_config(&config.storage)?;
    match super::registry_graph::with_stored_routing(
        config,
        Credentials::Skip,
        FileReferences::Warn,
    )
    .await
    {
        Ok(loaded) => {
            // A stored graph can gain a hosted registry while this replica
            // runs, so the store is bound to the database even while it
            // declares none.
            storage.ensure_hosted_layout().await?;
            super::release_record_logs::reconcile_configured_logs(&storage, &loaded).await?;
        }
        Err(err) => {
            tracing::warn!(
                error = %err,
                "could not load the stored registry graph; skipping the transparency log reconcile",
            );
            storage.ensure_hosted_layout().await?;
        }
    }
    claim_ephemeral_cache_dir(config).await?;
    storage.reset_cache_from_other_layout().await?;
    crate::cache_maintenance::sync_cache_secret_on_startup(config).await
}

/// Reclaim blob uploads abandoned before an unclean shutdown, and any left
/// by a client that never came back. Only image pushes create these, so this
/// is a no-op for a registry serving no hosted image ecosystem.
async fn sweep_abandoned_uploads(config: &Config) -> pnpr_error::Result<()> {
    let mut namespaces: Vec<&str> = config.routing.hosted
        .iter()
        .filter(|(name, _)| config.routing.registries.ecosystem(name) == Some(Ecosystem::Oci))
        .map(|(_, hosted)| hosted.namespace.as_str())
        .collect();
    if namespaces.is_empty() {
        return Ok(());
    }
    let storage = Storage::from_config(&config.storage)?;
    namespaces.sort_unstable();
    namespaces.dedup();
    for namespace in namespaces {
        let swept = storage
            .for_hosted(namespace, Ecosystem::Oci)
            .sweep_blob_uploads(pnpr_storage::upload::UPLOAD_MAX_AGE)
            .await?;
        if swept > 0 {
            tracing::info!(swept, namespace, "reclaimed abandoned blob uploads");
        }
    }
    Ok(())
}

/// Remove the archive staging files a process left behind when it stopped
/// between writing an archive and storing it, in every hosted namespace.
/// [`spawn_periodic_archive_temp_sweep`] repeats it while the server runs.
async fn sweep_abandoned_archive_temps(config: &Config) -> pnpr_error::Result<()> {
    let bases = pnpr_storage::archive_temp_bases(
        &config.storage,
        &hosted_archive_namespaces(&config.routing),
    );
    let max_age = crate::cache_maintenance::archive_staging_max_age(config);
    sweep_archive_temp_bases(&bases, max_age).await
}

/// Sweep abandoned archive staging files periodically, in the hosted
/// namespaces of the graph the server serves at each pass.
fn spawn_periodic_archive_temp_sweep(inner: &Arc<super::AppInner>) {
    let max_age = crate::cache_maintenance::archive_staging_max_age(&inner.config);
    let period =
        max_age.clamp(std::time::Duration::from_secs(30), std::time::Duration::from_mins(5));
    let inner = Arc::downgrade(inner);
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(period);
        interval.tick().await;
        loop {
            interval.tick().await;
            let Some(inner) = inner.upgrade() else { break };
            if let Err(err) = sweep_current_archive_temps(&inner).await {
                tracing::warn!(error = %err, "sweeping abandoned archive staging files failed");
            }
        }
    });
}

/// One periodic pass of [`spawn_periodic_archive_temp_sweep`].
async fn sweep_current_archive_temps(inner: &super::AppInner) -> pnpr_error::Result<()> {
    let routing = inner.current_routing();
    let namespaces = hosted_archive_namespaces(&routing);
    let bases = pnpr_storage::archive_temp_bases(&inner.config.storage, &namespaces);
    let max_age = crate::cache_maintenance::archive_staging_max_age(&inner.config);
    sweep_archive_temp_bases(&bases, max_age).await
}

fn hosted_archive_namespaces(routing: &RoutingConfig) -> Vec<(String, Ecosystem)> {
    let mut namespaces: Vec<(String, Ecosystem)> = routing.hosted
        .iter()
        .filter_map(|(name, hosted)| {
            Some((hosted.namespace.clone(), routing.registries.ecosystem(name)?))
        })
        .collect();
    namespaces.sort_unstable();
    namespaces.dedup();
    namespaces
}

async fn sweep_archive_temp_bases(
    bases: &[std::path::PathBuf],
    max_age: std::time::Duration,
) -> pnpr_error::Result<()> {
    for base in bases {
        let swept = pnpr_storage::sweep_stale_temps(base, max_age).await?;
        if swept.files > 0 {
            tracing::info!(
                swept = swept.files,
                bytes = swept.bytes,
                "removed abandoned archive staging files",
            );
        }
    }
    Ok(())
}

/// Remove release archives staged for a publish that never claimed them
/// before they expired, now and then every hour while the server runs.
async fn sweep_expired_staged_archives(config: &Config) -> pnpr_error::Result<()> {
    let storage = Storage::from_config(&config.storage)?;
    report_staged_archive_sweep(storage.sweep_staged_archives().await?);
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_hours(1));
        interval.tick().await;
        loop {
            interval.tick().await;
            match storage.sweep_staged_archives().await {
                Ok(swept) => report_staged_archive_sweep(swept),
                Err(err) => tracing::warn!(error = %err, "sweeping staged release archives failed"),
            }
        }
    });
    Ok(())
}

fn report_staged_archive_sweep(swept: usize) {
    if swept > 0 {
        tracing::info!(swept, "reclaimed expired staged release archives");
    }
}

const EXPIRED_TOKEN_SWEEP_INTERVAL: std::time::Duration = std::time::Duration::from_hours(1);

/// Delete expired personal tokens at startup and then every hour while the
/// server runs.
async fn sweep_expired_tokens(auth: &AuthState) {
    match auth.tokens.delete_expired(pnpr_auth::unix_millis()).await {
        Ok(swept) => report_expired_token_sweep(swept),
        Err(err) => tracing::warn!(error = %err, "sweeping expired tokens failed"),
    }
    let tokens = Arc::downgrade(&auth.tokens);
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(EXPIRED_TOKEN_SWEEP_INTERVAL);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        interval.tick().await;
        loop {
            interval.tick().await;
            let Some(tokens) = tokens.upgrade() else {
                break;
            };
            match tokens.delete_expired(pnpr_auth::unix_millis()).await {
                Ok(swept) => report_expired_token_sweep(swept),
                Err(err) => tracing::warn!(error = %err, "sweeping expired tokens failed"),
            }
        }
    });
}

fn report_expired_token_sweep(swept: usize) {
    if swept > 0 {
        tracing::info!(swept, "removed expired tokens");
    }
}

/// Run startup side effects and load the auth backends. The registry
/// prepares its storage; auth loads on every tier because the
/// account endpoints (which mint and manage tokens) are always served,
/// and every mounted surface consults caller identity.
async fn load_startup_auth(config: &Config, graph: StartupGraph) -> pnpr_error::Result<AuthState> {
    if config.features.registry.enabled {
        prepare_storage(config).await?;
        sweep_abandoned_uploads(config).await?;
        sweep_abandoned_archive_temps(config).await?;
        sweep_expired_staged_archives(config).await?;
    } else {
        claim_ephemeral_cache_dir(config).await?;
    }
    if graph == StartupGraph::Unavailable {
        tracing::warn!(
            "skipping the startup cache pruning: without the stored registry graph every upstream \
             cache would look orphaned",
        );
    } else if config.features.registry.enabled || config.features.resolver.enabled {
        crate::cache_maintenance::prune_orphaned_namespaces_startup(config).await?;
    } else {
        crate::cache_maintenance::sync_cache_secret_on_startup(config).await?;
    }
    let auth = AuthState::load(&config.identity.auth, &config.identity.backend).await?;
    sweep_expired_tokens(&auth).await;
    Ok(auth)
}

/// Start from the stored graph, seeding the default one into an empty
/// database, so the startup work over the hosted registries sees them. A graph
/// that cannot be loaded leaves the replica serving none until a reload
/// succeeds.
async fn load_stored_graph(config: &mut Config) -> StartupGraph {
    if config.graph_source == GraphSource::Supplied {
        return StartupGraph::Supplied;
    }
    if let Err(err) = seed(config).await {
        tracing::error!(error = %err, "could not store the default registries");
        return StartupGraph::Unavailable;
    }
    match super::registry_graph::load_stored_routing(
        config,
        Credentials::Resolve,
        FileReferences::Warn,
    )
    .await
    {
        Ok((routing, generation)) => {
            config.routing = routing;
            StartupGraph::Stored { generation }
        }
        Err(err) => {
            tracing::error!(error = %err, "could not load the stored registry graph");
            StartupGraph::Unavailable
        }
    }
}

/// Serve the stored graph startup loaded as the confirmed one, so the stale
/// limit counts from startup instead of from the first reload.
fn adopt_startup_graph(state: &AppState, graph: StartupGraph) {
    if let StartupGraph::Stored { generation } = graph {
        let routing = RoutingConfig::clone(&state.inner.current_routing());
        drop(super::registry_graph::publish_routing(state, routing, generation));
    }
}

async fn seed(config: &Config) -> Result<(), pnpr_error::RegistryError> {
    let storage = Storage::from_config(&config.storage)?;
    super::registry_graph::seed_default_graph(&storage).await
}

/// Whether startup holds the registry graph the server will serve.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StartupGraph {
    /// The embedder supplied the graph in code.
    Supplied,
    /// The stored graph of `generation` loaded.
    Stored { generation: Option<u64> },
    /// The stored graph could not be loaded; startup work that walks the
    /// registries skips what it would get wrong without them.
    Unavailable,
}

/// Bind to `config.http.listen` and serve forever. Loads auth state before
/// binding so a startup-time auth error surfaces before we accept any
/// client connections. Registry startup additionally prepares the storage.
pub async fn serve(mut config: Config) -> pnpr_error::Result<()> {
    // Enforce the "at least one surface" invariant here too, not only at
    // YAML load / CLI: embedders build `Config` programmatically and call
    // straight into `serve`, so a both-disabled config must fail loudly
    // rather than start a server that only answers `/-/ping`.
    crate::config_validation::validate_serving_startup_config(&mut config)?;
    log_enabled_surfaces(&config);
    let graph = load_stored_graph(&mut config).await;
    let osv_index = load_active_osv_index(&config)?;
    let auth = load_startup_auth(&config, graph).await?;
    let listen = config.http.listen;
    // Build the router before taking the port: it can fail, and a failure
    // should not leave a bound socket behind or put a `pnpr listening` line
    // immediately above the error saying it is not.
    let (state, app) = state_and_router(config, auth, osv_index)?;
    adopt_startup_graph(&state, graph);
    let inner = Arc::clone(&state.inner);
    let metrics = bind_metrics_listener(state.clone()).await?;
    let listener = tokio::net::TcpListener::bind(listen).await?;
    let _cache_eviction = crate::cache_maintenance::spawn_periodic_cache_eviction(
        &inner.config,
        current_routing_of(&inner),
    );
    if inner.config.features.registry.enabled {
        spawn_periodic_archive_temp_sweep(&inner);
    }
    tracing::info!(listen = %listener.local_addr()?, "pnpr listening");
    serve_until_shutdown(state, listener, app, metrics).await
}

/// The registry graph `inner` serves at each call: the routing a periodic
/// cache pass protects. `inner.config.routing` holds none of it. `None` while
/// the server cannot serve its stored graph.
fn current_routing_of(
    inner: &Arc<super::AppInner>,
) -> impl Fn() -> Option<RoutingConfig> + Send + 'static {
    let inner = Arc::clone(inner);
    move || {
        let available = inner.identity.managed.graph
            .as_ref()
            .is_none_or(super::registry_graph::GraphState::is_available);
        available.then(|| RoutingConfig::clone(&inner.current_routing()))
    }
}

/// The listener `metrics.listen` names, with the router it serves.
struct MetricsListener {
    listener: tokio::net::TcpListener,
    router: Router,
}

async fn bind_metrics_listener(state: AppState) -> pnpr_error::Result<Option<MetricsListener>> {
    let Some(listen) = state.inner.config.http.metrics.as_ref().and_then(|metrics| metrics.listen)
    else {
        return Ok(None);
    };
    let listener = tokio::net::TcpListener::bind(listen).await
        .map_err(|err| {
            std::io::Error::new(err.kind(), format!("cannot bind metrics.listen {listen}: {err}"))
        })?;
    tracing::info!(listen = %listener.local_addr()?, "pnpr serving metrics");
    Ok(Some(MetricsListener { listener, router: metrics_listener_router(state) }))
}

async fn serve_until_shutdown(
    state: AppState,
    listener: tokio::net::TcpListener,
    app: Router,
    metrics: Option<MetricsListener>,
) -> pnpr_error::Result<()> {
    serve_until_shutdown_with(state, listener, app, metrics, shutdown_signal()).await
}

async fn serve_until_shutdown_with(
    state: AppState,
    listener: tokio::net::TcpListener,
    app: Router,
    metrics: Option<MetricsListener>,
    signal: impl std::future::Future<Output = ()> + Send + 'static,
) -> pnpr_error::Result<()> {
    let drain_timeout = state.inner.config.http.shutdown_timeout;
    let shutdown = async move {
        signal.await;
        state.mark_shutting_down();
    }
    .shared();
    let main = axum::serve(
        NodelayTcpListener(listener),
        app.into_make_service_with_connect_info::<PeerAddr>(),
    )
    .with_graceful_shutdown(shutdown.clone())
    .into_future();
    let deadline_signal = shutdown.clone();
    let serving = async {
        match metrics {
            None => main.await?,
            Some(MetricsListener { listener, router }) => {
                let metrics = axum::serve(listener, router)
                    .with_graceful_shutdown(shutdown.clone())
                    .into_future();
                tokio::try_join!(main, metrics)?;
            }
        }
        Ok(())
    };
    drain_until_deadline(serving, deadline_signal, drain_timeout).await
}

async fn drain_until_deadline(
    serving: impl std::future::Future<Output = pnpr_error::Result<()>>,
    shutdown: impl std::future::Future<Output = ()>,
    drain_timeout: std::time::Duration,
) -> pnpr_error::Result<()> {
    tokio::pin!(serving);
    tokio::select! {
        result = &mut serving => return result,
        () = shutdown => {}
    }
    if let Ok(result) = tokio::time::timeout(drain_timeout, serving).await {
        result
    } else {
        tracing::warn!(
            timeout = ?drain_timeout,
            "Shutdown drain exceeded timeout; stopping the server",
        );
        Ok(())
    }
}

/// Log which surfaces are mounted at startup. A misconfiguration — a
/// `registries:` block that didn't parse the way the operator meant, or a
/// typo'd `resolver:` block name, which the intentionally
/// verdaccio-lenient config parser silently ignores and so leaves the
/// surface at its default-enabled state — is then immediately visible to
/// the operator rather than only discoverable by probing.
fn log_enabled_surfaces(config: &Config) {
    tracing::info!(
        registry = config.features.registry.enabled,
        resolver = config.features.resolver.enabled,
        artifacts = config.features.artifacts.enabled,
        pipeline = config.features.pipeline.enabled,
        "pnpr surfaces",
    );
}

/// Serve on an already-bound listener.
///
/// Test harnesses can bind to `127.0.0.1:0`, read the OS-assigned
/// address, and then hand that listener here without a bind/drop/rebind
/// race.
pub async fn serve_listener(
    config: Config,
    listener: tokio::net::TcpListener,
) -> pnpr_error::Result<()> {
    serve_listener_with(config, listener, std::convert::identity).await
}

/// Like [`serve_listener`], serving `wrap` applied to the router, such as a
/// test harness that adds its own middleware.
pub async fn serve_listener_with(
    mut config: Config,
    listener: tokio::net::TcpListener,
    wrap: impl FnOnce(Router) -> Router,
) -> pnpr_error::Result<()> {
    let listen = listener.local_addr()?;
    crate::config_validation::validate_serving_startup_config(&mut config)?;
    log_enabled_surfaces(&config);
    let graph = load_stored_graph(&mut config).await;
    let osv_index = load_active_osv_index(&config)?;
    // Load the configured auth backends here too — going through `router`
    // would silently fall back to in-memory auth and ignore a persisted
    // htpasswd / SQLite store or a configured `backend:`.
    let auth = load_startup_auth(&config, graph).await?;
    let (state, app) = state_and_router(config, auth, osv_index)?;
    adopt_startup_graph(&state, graph);
    let inner = Arc::clone(&state.inner);
    let metrics = bind_metrics_listener(state.clone()).await?;
    let _cache_eviction = crate::cache_maintenance::spawn_periodic_cache_eviction(
        &inner.config,
        current_routing_of(&inner),
    );
    if inner.config.features.registry.enabled {
        spawn_periodic_archive_temp_sweep(&inner);
    }
    tracing::info!(%listen, "pnpr listening");
    serve_until_shutdown(state, listener, wrap(app), metrics).await
}

#[cfg(test)]
mod tests;
