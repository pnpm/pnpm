use std::future::IntoFuture;

use axum::Router;
use futures_util::FutureExt as _;

use super::{
    AppState, Arc, AuthState, Config, Ecosystem, NodelayTcpListener, PeerAddr, RegistryDocuments,
    Storage, metrics_listener_router, shutdown_signal, state_and_router,
};

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

/// Check the hosted store's layout, then bring the publish journal to a
/// consistent state: apply every sealed transaction, discard every unsealed
/// one. [`serve`] and [`serve_listener`] call this before binding; an embedder
/// that builds a router directly should call it itself on startup, before
/// serving requests.
pub async fn recover_publish_journal(config: &Config) -> pnpr_error::Result<()> {
    let storage = Storage::from_config(&config.storage)?;
    storage.ensure_hosted_layout().await?;
    storage.reset_cache_from_other_layout().await?;
    pnpr_storage::journal::recover_publish_journal(config, &RegistryDocuments).await
}

/// Reclaim blob uploads abandoned before an unclean shutdown, and any left
/// by a client that never came back. Only image pushes create these, so this
/// is a no-op for a registry serving no image ecosystem.
async fn sweep_abandoned_uploads(config: &Config) -> pnpr_error::Result<()> {
    if !config.routing.registries.has_ecosystem(Ecosystem::Oci) {
        return Ok(());
    }
    let storage = Storage::from_config(&config.storage)?;
    // Shared sessions live in their hosted namespace. Local scratch can be
    // shared by those namespaces and is safe to sweep more than once.
    let mut namespaces: Vec<&str> = config.routing.hosted
        .iter()
        .filter(|(name, _)| config.routing.registries.ecosystem(name) == Some(Ecosystem::Oci))
        .map(|(_, hosted)| hosted.org.as_str())
        .collect();
    namespaces.push("");
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

/// Run startup side effects and load the auth backends. The registry
/// needs publish-journal recovery; auth loads on every tier because the
/// account endpoints (which mint and manage tokens) are always served,
/// and every mounted surface consults caller identity.
async fn load_startup_auth(config: &Config) -> pnpr_error::Result<AuthState> {
    if config.features.registry.enabled {
        recover_publish_journal(config).await?;
        sweep_abandoned_uploads(config).await?;
        crate::cache_maintenance::prune_orphaned_namespaces_startup(config).await?;
    }
    AuthState::load(&config.identity.auth, &config.identity.backend).await
}

/// Bind to `config.http.listen` and serve forever. Loads auth state before
/// binding so a startup-time auth error surfaces before we accept any
/// client connections. Registry startup additionally recovers the publish
/// journal.
pub async fn serve(mut config: Config) -> pnpr_error::Result<()> {
    // Enforce the "at least one surface" invariant here too, not only at
    // YAML load / CLI: embedders build `Config` programmatically and call
    // straight into `serve`, so a both-disabled config must fail loudly
    // rather than start a server that only answers `/-/ping`.
    config.features.ensure_a_feature_is_enabled()?;
    config.routing.ensure_valid_registry_graph(config.features.registry.enabled)?;
    config.http.ensure_valid_metrics()?;
    log_enabled_surfaces(&config);
    let osv_index = load_active_osv_index(&config)?;
    let auth = load_startup_auth(&config).await?;
    let listen = config.http.listen;
    // Build the router before taking the port: it can fail, and a failure
    // should not leave a bound socket behind or put a `pnpr listening` line
    // immediately above the error saying it is not.
    crate::cache_maintenance::spawn_periodic_cache_eviction(&config);
    let (state, app) = state_and_router(config, auth, osv_index)?;
    let metrics = bind_metrics_listener(state).await?;
    let listener = tokio::net::TcpListener::bind(listen).await?;
    tracing::info!(%listen, "pnpr listening");
    serve_until_shutdown(listener, app, metrics).await
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
    listener: tokio::net::TcpListener,
    app: Router,
    metrics: Option<MetricsListener>,
) -> pnpr_error::Result<()> {
    let shutdown = shutdown_signal().shared();
    let main = axum::serve(
        NodelayTcpListener(listener),
        app.into_make_service_with_connect_info::<PeerAddr>(),
    )
    .with_graceful_shutdown(shutdown.clone())
    .into_future();
    match metrics {
        None => main.await?,
        Some(MetricsListener { listener, router }) => {
            let metrics =
                axum::serve(listener, router).with_graceful_shutdown(shutdown).into_future();
            tokio::try_join!(main, metrics)?;
        }
    }
    Ok(())
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
    mut config: Config,
    listener: tokio::net::TcpListener,
) -> pnpr_error::Result<()> {
    let listen = listener.local_addr()?;
    config.features.ensure_a_feature_is_enabled()?;
    config.routing.ensure_valid_registry_graph(config.features.registry.enabled)?;
    config.http.ensure_valid_metrics()?;
    log_enabled_surfaces(&config);
    let osv_index = load_active_osv_index(&config)?;
    // Load the configured auth backends here too — going through `router`
    // would silently fall back to in-memory auth and ignore a persisted
    // htpasswd / SQLite store or a configured `backend:`.
    let auth = load_startup_auth(&config).await?;
    let (state, app) = state_and_router(config, auth, osv_index)?;
    let metrics = bind_metrics_listener(state).await?;
    tracing::info!(%listen, "pnpr listening");
    serve_until_shutdown(listener, app, metrics).await
}
