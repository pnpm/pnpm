use super::{
    Arc, AuthState, Config, Ecosystem, NodelayTcpListener, PeerAddr, RegistryDocuments, Storage,
    router_with_auth_and_osv, shutdown_signal,
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
    let storage = Storage::new(
        &config.storage.hosted_backend,
        config.storage.hosted_dir.clone(),
        config.storage.cache_dir.clone(),
    )?;
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
    let storage = Storage::new(
        &config.storage.hosted_backend,
        config.storage.hosted_dir.clone(),
        config.storage.cache_dir.clone(),
    )?;
    // Shared sessions live in their hosted namespace. Local scratch can be
    // shared by those namespaces and is safe to sweep more than once.
    let mut namespaces: Vec<&str> = config.routing.hosted
        .values()
        .map(|hosted| hosted.org.as_str())
        .collect();
    namespaces.push("");
    namespaces.sort_unstable();
    namespaces.dedup();
    for namespace in namespaces {
        let swept = storage
            .for_hosted(namespace)
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
    log_enabled_surfaces(&config);
    let osv_index = load_active_osv_index(&config)?;
    let auth = load_startup_auth(&config).await?;
    let listen = config.http.listen;
    // Build the router before taking the port: it can fail, and a failure
    // should not leave a bound socket behind or put a `pnpr listening` line
    // immediately above the error saying it is not.
    crate::cache_maintenance::spawn_periodic_cache_eviction(&config);
    let app = router_with_auth_and_osv(config, auth, osv_index)?;
    let listener = NodelayTcpListener(tokio::net::TcpListener::bind(listen).await?);
    tracing::info!(%listen, "pnpr listening");
    axum::serve(listener, app.into_make_service_with_connect_info::<PeerAddr>())
        .with_graceful_shutdown(shutdown_signal())
        .await?;
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
    log_enabled_surfaces(&config);
    let osv_index = load_active_osv_index(&config)?;
    // Load the configured auth backends here too — going through `router`
    // would silently fall back to in-memory auth and ignore a persisted
    // htpasswd / SQLite store or a configured `backend:`.
    let auth = load_startup_auth(&config).await?;
    let app = router_with_auth_and_osv(config, auth, osv_index)?;
    tracing::info!(%listen, "pnpr listening");
    axum::serve(
        NodelayTcpListener(listener),
        app.into_make_service_with_connect_info::<PeerAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await?;
    Ok(())
}
