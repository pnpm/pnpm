use std::{sync::Arc, time::Duration};

use axum::{
    Router,
    body::Body,
    extract::{DefaultBodyLimit, OriginalUri, Path, Request, State},
    http::HeaderMap,
    middleware,
    response::Response,
    routing::{any, delete, get, post, put},
};
use indexmap::IndexMap;
use tower_http::{
    compression::{
        CompressionLayer,
        predicate::{DefaultPredicate, NotForContentType, Predicate as _},
    },
    trace::TraceLayer,
};
use tracing::Span;

use pnpr_auth::AuthState;
use pnpr_config::Config;
use pnpr_storage::Storage;
use pnpr_upstream::Upstream;

use super::{
    AppInner, AppState, AuthedCaller, MAX_ARTIFACT_BLOB_BODY_BYTES,
    MAX_ARTIFACT_PUBLISH_BODY_BYTES, MAX_ARTIFACT_RESOLVE_BODY_BYTES, MAX_LOGIN_BODY_BYTES,
    MAX_PUBLISH_BODY_BYTES, StripedLocks, authenticate, compute_upstream_cache_namespace,
    default_registry_target, delete_package, delete_session_token, delete_tarball,
    delete_token_by_key, get_dist_tags, get_org_teams, get_profile, get_team_members,
    get_token_list, get_whoami, loggable_uri, not_found, pnpr_protocols_disabled,
    private_if_caller_gated, private_no_cache, publish_package, put_login, reject_team_mutation,
    remove_dist_tag, require_artifact_caller, require_resolver_caller, serve_artifact_blob,
    serve_batch_publish, serve_packument, serve_ping, serve_pnpr_handshake, serve_publish_artifact,
    serve_registry_packument, serve_registry_tarball, serve_registry_version_manifest,
    serve_resolve, serve_resolve_artifacts, serve_revision_tarball, serve_search, serve_tarball,
    serve_verify_lockfile, serve_version_manifest, set_dist_tag, staged, tilde_registry,
    update_packument, upstream_tarball_base,
};

pub(super) fn router_with_auth_and_osv(
    config: Config,
    auth: AuthState,
    osv_index: Option<Arc<pnpr_osv::OsvIndex>>,
) -> pnpr_error::Result<Router> {
    let storage =
        Storage::new(&config.hosted_store, config.storage.clone(), config.cache_storage.clone())?;
    let registry_enabled = config.registry.enabled;
    let resolver_enabled = config.resolver.enabled;
    let artifacts_enabled = config.artifacts.enabled;
    let artifacts = artifacts_enabled
        .then(|| {
            pnpr_shared_artifacts::SharedArtifactStore::new(
                &config.hosted_store,
                &config.cache_storage,
            )
        })
        .transpose()?;
    // Only the registry routes consult the upstreams, so a resolver-only
    // server builds none — skipping a `ThrottledClient` allocation per
    // configured upstream.
    let upstreams: IndexMap<String, Upstream> = if registry_enabled {
        config
            .upstreams
            .iter()
            .map(|(name, upstream)| (name.clone(), Upstream::new(name, upstream)))
            .collect()
    } else {
        IndexMap::new()
    };
    let upstream_cache_namespaces = config
        .upstreams
        .keys()
        .map(|name| (name.clone(), compute_upstream_cache_namespace(&config, name)))
        .collect();
    let state = AppState {
        inner: Arc::new(AppInner {
            storage,
            artifacts,
            upstreams,
            upstream_cache_namespaces,
            config,
            auth,
            package_locks: StripedLocks::new(),
            resolver: std::sync::OnceLock::new(),
            osv_index,
        }),
    };
    // `/-/ping` is a health check and is always served. The two
    // configurable surfaces are mounted only when their feature is enabled,
    // so resolver, registry, and artifacts can be deployed independently.
    // The config guarantees at least one is enabled.
    let mut router = Router::new().route("/-/ping", get(serve_ping));
    // The account endpoints — adduser/login, whoami, profile, token
    // listing/revocation, logout — are pnpr account management, not
    // npm-registry functionality: they mint and manage the tokens every
    // authenticated surface demands, so they ride every tier alongside
    // `/-/ping`. A resolver- or artifacts-only tier can then issue its own credentials
    // (`pnpm login --registry https://<resolver-host>/`) instead of
    // depending on a registry-serving replica that shares the auth backend.
    //
    // Each endpoint also answers under any `/~<prefix>/`, so a client whose
    // registry URL is a registry endpoint can log in against it. The identity
    // endpoints are global and consult no registry state; a registry-table lookup
    // would gate nothing while turning the 401-vs-404 split into an
    // existence oracle for private registry names that the content handlers
    // carefully mask.
    router = router
        .route("/-/whoami", get(get_whoami))
        .route("/{prefix}/-/whoami", get(get_whoami))
        .route(
            "/-/user/{user}",
            put(put_login).route_layer(DefaultBodyLimit::max(MAX_LOGIN_BODY_BYTES)),
        )
        .route(
            "/{prefix}/-/user/{user}",
            put(put_login).route_layer(DefaultBodyLimit::max(MAX_LOGIN_BODY_BYTES)),
        )
        .route("/-/user/token/{token}", delete(delete_session_token))
        .route("/{prefix}/-/user/token/{token}", delete(delete_session_token))
        .route("/-/npm/v1/user", get(get_profile))
        .route("/{prefix}/-/npm/v1/user", get(get_profile))
        .route("/-/npm/v1/tokens", get(get_token_list))
        .route("/{prefix}/-/npm/v1/tokens", get(get_token_list))
        .route("/-/npm/v1/tokens/token/{key}", delete(delete_token_by_key))
        .route("/{prefix}/-/npm/v1/tokens/token/{key}", delete(delete_token_by_key));
    // The install-accelerator and shared-artifact surfaces live under the
    // reserved `/-/pnpr` namespace. The handshake advertises each protocol
    // independently, so either surface can be mounted on its own.
    //
    // When both protocol surfaces are disabled, only `/-/pnpr` gets a 404
    // stub: it is the capability-probe path and overlaps the registry catch-all
    // (`/-/pnpr` matches `/{first}/{second}`), so without the stub a probe
    // would be proxied upstream, giving a confusing 502 where a client
    // expects the "no pnpr protocols here" 404. The `/-/pnpr/v0/*` endpoints
    // carry no capability probe, so they are left unmounted rather than
    // stubbed.
    if resolver_enabled || artifacts_enabled {
        router = router.route("/-/pnpr", get(serve_pnpr_handshake));
    } else {
        router = router.route("/-/pnpr", any(pnpr_protocols_disabled));
    }
    if resolver_enabled {
        router = router
            .route(
                "/-/pnpr/v0/resolve",
                post(serve_resolve).route_layer(middleware::from_fn_with_state(
                    state.clone(),
                    require_resolver_caller,
                )),
            )
            .route(
                "/-/pnpr/v0/verify-lockfile",
                post(serve_verify_lockfile).route_layer(middleware::from_fn_with_state(
                    state.clone(),
                    require_resolver_caller,
                )),
            );
    }
    if artifacts_enabled {
        router = router
            .route(
                "/-/pnpr/v0/artifacts",
                put(serve_publish_artifact)
                    .route_layer(DefaultBodyLimit::max(MAX_ARTIFACT_PUBLISH_BODY_BYTES))
                    .route_layer(middleware::from_fn_with_state(
                        state.clone(),
                        require_artifact_caller,
                    )),
            )
            .route(
                "/-/pnpr/v0/artifacts/resolve",
                post(serve_resolve_artifacts)
                    .route_layer(DefaultBodyLimit::max(MAX_ARTIFACT_RESOLVE_BODY_BYTES))
                    .route_layer(middleware::from_fn_with_state(
                        state.clone(),
                        require_artifact_caller,
                    )),
            )
            .route(
                "/-/pnpr/v0/artifacts/blob",
                post(serve_artifact_blob)
                    .route_layer(DefaultBodyLimit::max(MAX_ARTIFACT_BLOB_BODY_BYTES))
                    .route_layer(middleware::from_fn_with_state(
                        state.clone(),
                        require_artifact_caller,
                    )),
            );
    }
    // The npm-registry surface: every packument/tarball read, publish,
    // unpublish, dist-tag, and search. When the surface is off (no registries
    // declared, or `--disable-registry`), none of these routes are mounted.
    // Resolver- and artifacts-only tiers expose no registry surface at all.
    if registry_enabled {
        router = router
            // Batch publish: one request carrying many packages' publish
            // documents. Not part of the standard npm registry API —
            // `pnpm publish --batch` opts into it explicitly.
            .route("/-/pnpm/v1/publish", put(serve_batch_publish))
            // Staged (two-phase) publishing — the `pnpm stage` surface.
            // Static `-`/`stage` segments take priority over the generic
            // segment-count routes below, so these never shadow package
            // reads. Each route has a `/~<name>/`-prefixed twin so a client
            // whose registry URL is a registry endpoint can stage through it.
            .route("/-/stage", get(staged::list_staged))
            .route("/-/stage/package/{name}", post(staged::post_staged_publish))
            .route("/-/stage/{id}", get(staged::get_staged).delete(staged::reject_staged))
            .route("/-/stage/{id}/approve", post(staged::approve_staged))
            .route("/-/stage/{id}/tarball", get(staged::get_staged_tarball))
            .route("/{prefix}/-/stage", get(staged::list_staged))
            .route("/{prefix}/-/stage/package/{name}", post(staged::post_staged_publish))
            .route("/{prefix}/-/stage/{id}", get(staged::get_staged).delete(staged::reject_staged))
            .route("/{prefix}/-/stage/{id}/approve", post(staged::approve_staged))
            .route("/{prefix}/-/stage/{id}/tarball", get(staged::get_staged_tarball))
            .route("/{name}", get(get_packument_unscoped).put(put_one_segment))
            .route("/{first}/{second}", get(get_two_segments).put(put_two_segments))
            .route(
                "/{first}/{second}/{third}",
                get(get_three_segments).put(put_three_segments).delete(delete_three_segments),
            )
            .route("/{scope}/{name}/-/{filename}", get(get_tarball_scoped))
            .route(
                "/{a}/{b}/{c}/{d}",
                get(get_four_segments).put(put_four_segments).delete(delete_four_segments),
            )
            .route(
                "/{a}/{b}/{c}/{d}/{e}",
                get(get_five_segments).put(put_five_segments).delete(delete_five_segments),
            )
            // Scoped tarball delete: `DELETE /@scope/name/-/<basename-version>.tgz/-rev/<rev>`,
            // plus the registry-addressed dist-tag write and unscoped tarball delete.
            .route(
                "/{a}/{b}/{c}/{d}/{e}/{f}",
                get(get_six_segments).put(put_six_segments).delete(delete_six_segments),
            )
            // Registry-addressed scoped tarball delete:
            // `DELETE /~<name>/@scope/name/-/<file>/-rev/<rev>`
            .route("/{a}/{b}/{c}/{d}/{e}/{f}/{g}", delete(delete_seven_segments));
    }
    Ok(router
        .layer(DefaultBodyLimit::max(MAX_PUBLISH_BODY_BYTES))
        // Authenticate once, ahead of every handler: resolve the caller,
        // enforce bearer-token read-only / CIDR restrictions (so a
        // restricted token is rejected before a write handler buffers its
        // up-to-100-MiB body), and stash the identity for handlers to read.
        // Inside the trace layer below, so a rejection is still one record.
        .layer(axum::middleware::from_fn_with_state(state.clone(), authenticate))
        // gzip metadata responses for clients that send `Accept-Encoding:
        // gzip`, matching how a real (CDN-fronted) registry serves
        // packuments — pnpr is commonly hit directly with no proxy in
        // front, so the application is the only layer that can compress.
        // Scoped to JSON: tarballs (`application/octet-stream`, already
        // `.tgz`) are excluded so we never re-gzip an already-compressed
        // payload. The pnpr resolver NDJSON streams
        // (`application/x-ndjson`) is excluded too: gzip-buffering it
        // would defeat the point of streaming — frames must flush to the
        // client as each package resolves, not wait for the encoder.
        .layer(
            CompressionLayer::new().compress_when(
                DefaultPredicate::new()
                    .and(NotForContentType::const_new("application/octet-stream"))
                    .and(NotForContentType::const_new("application/x-ndjson")),
            ),
        )
        // One structured access record per HTTP request: a span
        // carrying method + URI plus a single `finished processing
        // request` event on the response with status and latency.
        // Both the span and the event use the `pnpr::access`
        // target so `LogLevel::Http`'s filter directive can scope to
        // them. `on_request(())` / `on_failure(())` suppress
        // tower-http's default emissions so each request produces
        // exactly one record. The format and level are picked up from
        // the subscriber installed in `main.rs` (driven by the YAML
        // `log:` block — pretty or NDJSON).
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(|request: &Request<Body>| {
                    tracing::info_span!(
                        target: "pnpr::access",
                        "request",
                        method = %request.method(),
                        uri = %loggable_uri(request.uri()),
                        // Filled in by `record_cache_status` for packument
                        // reads (e.g. `cache=hit`); stays absent otherwise.
                        cache = tracing::field::Empty,
                    )
                })
                .on_request(())
                .on_response(|response: &Response<Body>, latency: Duration, _span: &Span| {
                    tracing::info!(
                        target: "pnpr::access",
                        status = response.status().as_u16(),
                        latency_ms = latency.as_millis() as u64,
                        "finished processing request",
                    );
                })
                .on_failure(()),
        )
        .with_state(state))
}

// --------------------------------------------------------------------
// GET handlers — packument, version manifest, tarball.
// Same overall shape as before, with an access-policy check added
// up front so protected packages return 401 to anonymous callers.
// --------------------------------------------------------------------

async fn get_packument_unscoped(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    headers: HeaderMap,
    Path(name): Path<String>,
) -> Response {
    serve_packument(&state, &identity, &headers, &name).await
}

async fn get_two_segments(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    headers: HeaderMap,
    Path((first, second)): Path<(String, String)>,
) -> Response {
    // `/~<name>/<pkg>` — unscoped packument through a registry endpoint. The
    // tarball base is the client's `/~<name>/` URL so the rewritten URLs stay
    // canonical for the registry the client actually addressed.
    if let Some(registry) = tilde_registry(&first) {
        let base = upstream_tarball_base(&state.inner.config.public_url, registry);
        return private_no_cache(
            serve_registry_packument(&state, &identity, &headers, registry, &second, &base).await,
        );
    }
    if first.starts_with('@') {
        if first.contains('/') {
            serve_version_manifest(&state, &identity, &first, &second).await
        } else {
            let full = format!("{first}/{second}");
            serve_packument(&state, &identity, &headers, &full).await
        }
    } else {
        serve_version_manifest(&state, &identity, &first, &second).await
    }
}

async fn get_three_segments(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    OriginalUri(uri): OriginalUri,
    headers: HeaderMap,
    Path((first, second, third)): Path<(String, String, String)>,
) -> Response {
    if first == "-" && second == "v1" && third == "search" {
        let query = uri.query().unwrap_or("");
        // Search results are filtered per caller (registry access + per-package
        // ACL), so they must never land in a shared HTTP cache.
        return private_no_cache(serve_search(&state, &identity, None, query).await);
    }
    if let Some(registry) = tilde_registry(&first) {
        // The account endpoints (whoami, adduser, logout, profile, tokens)
        // live on dedicated always-mounted routes; a `/~<name>/-/...` path
        // that still reaches this handler names no registry content.
        if second == "-" {
            return not_found();
        }
        let base = upstream_tarball_base(&state.inner.config.public_url, registry);
        if second.starts_with('@') {
            // `/~<name>/@scope%2Fname/<version>` — version manifest for an
            // encoded scoped package through a registry endpoint.
            if second.contains('/') {
                return private_no_cache(
                    serve_registry_version_manifest(
                        &state, &identity, registry, &second, &third, &base,
                    )
                    .await,
                );
            }
            // `/~<name>/@scope/<pkg>` — scoped packument through a registry.
            let full = format!("{second}/{third}");
            return private_no_cache(
                serve_registry_packument(&state, &identity, &headers, registry, &full, &base).await,
            );
        }
        // `/~<name>/<pkg>/<version-or-tag>` — unscoped version manifest
        // through a registry endpoint. (The unscoped tarball shape
        // `/~<name>/<pkg>/-/<file>` is a distinct literal-`-` route.)
        return private_no_cache(
            serve_registry_version_manifest(&state, &identity, registry, &second, &third, &base)
                .await,
        );
    }
    if second == "-" {
        serve_tarball(&state, &identity, &first, &third).await
    } else if first.starts_with('@') {
        let full = format!("{first}/{second}");
        serve_version_manifest(&state, &identity, &full, &third).await
    } else {
        not_found()
    }
}

async fn get_tarball_scoped(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    Path((scope, name, filename)): Path<(String, String, String)>,
) -> Response {
    // `/~<name>/<pkg>/-/<file>` — unscoped tarball through a registry endpoint.
    if let Some(registry) = tilde_registry(&scope) {
        return private_no_cache(
            serve_registry_tarball(&state, &identity, registry, &name, &filename).await,
        );
    }
    if !scope.starts_with('@') {
        return not_found();
    }
    let full = format!("{scope}/{name}");
    serve_tarball(&state, &identity, &full, &filename).await
}

/// 4-segment GET:
/// * `/-/package/{pkg}/dist-tags` — packument's `dist-tags` object.
/// * `/-/org/{scope}/team` — the teams of the registry claiming `@scope`.
/// * `/-/tarballs/sha512/{digest}` — immutable artifact through the default registry.
/// * `/~<name>/-/v1/search` — search through a registry endpoint.
/// * `/~<name>/@scope/{pkg}/{version}` — scoped version manifest through a
///   registry endpoint.
async fn get_four_segments(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    OriginalUri(uri): OriginalUri,
    Path((a, b, c, d)): Path<(String, String, String, String)>,
) -> Response {
    if a == "-" && b == "tarballs" && c == "sha512" {
        let Some(registry) = default_registry_target(&state) else { return not_found() };
        return serve_revision_tarball(&state, &identity, &registry, &d).await;
    }
    if a == "-" && b == "package" && d == "dist-tags" {
        let response = get_dist_tags(&state, &identity, None, &c).await;
        return private_if_caller_gated(&state, &c, response);
    }
    if a == "-" && b == "org" && d == "team" {
        return private_no_cache(get_org_teams(&state, &identity, None, &c));
    }
    if let Some(registry) = tilde_registry(&a) {
        if b == "-" && c == "v1" && d == "search" {
            let query = uri.query().unwrap_or("");
            return private_no_cache(serve_search(&state, &identity, Some(registry), query).await);
        }
        if b.starts_with('@') && !b.contains('/') {
            let full = format!("{b}/{c}");
            let base = upstream_tarball_base(&state.inner.config.public_url, registry);
            return private_no_cache(
                serve_registry_version_manifest(&state, &identity, registry, &full, &d, &base)
                    .await,
            );
        }
    }
    not_found()
}

/// 5-segment GET:
/// * `/-/team/{scope}/{team}/user` — a team's members.
/// * `/~<name>/@scope/<pkg>/-/<file>` — scoped tarball through a registry
///   endpoint.
/// * `/~<name>/-/package/<pkg>/dist-tags` — dist-tags through a registry
///   endpoint.
/// * `/~<name>/-/org/{scope}/team` — org teams through a registry endpoint.
/// * `/~<name>/-/tarballs/sha512/{digest}`: immutable artifact through an addressed registry.
///
/// Every other 5-segment GET is a not-found catchall (the route exists so
/// DELETE/PUT can sit on the same path).
async fn get_five_segments(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    Path((a, b, c, d, e)): Path<(String, String, String, String, String)>,
) -> Response {
    if a == "-" && b == "team" && e == "user" {
        return private_no_cache(get_team_members(&state, &identity, None, &c, &d));
    }
    if let Some(registry) = tilde_registry(&a) {
        if b == "-" && c == "tarballs" && d == "sha512" {
            return serve_revision_tarball(&state, &identity, registry, &e).await;
        }
        if b.starts_with('@') && d == "-" {
            let full = format!("{b}/{c}");
            return private_no_cache(
                serve_registry_tarball(&state, &identity, registry, &full, &e).await,
            );
        }
        if b == "-" && c == "package" && e == "dist-tags" {
            return private_no_cache(get_dist_tags(&state, &identity, Some(registry), &d).await);
        }
        if b == "-" && c == "org" && e == "team" {
            return private_no_cache(get_org_teams(&state, &identity, Some(registry), &d));
        }
    }
    not_found()
}

/// 6-segment GET:
/// * `/~<name>/-/team/{scope}/{team}/user` — a team's members through a
///   registry endpoint.
async fn get_six_segments(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    Path((a, b, c, d, e, f)): Path<(String, String, String, String, String, String)>,
) -> Response {
    if let Some(registry) = tilde_registry(&a)
        && b == "-"
        && c == "team"
        && f == "user"
    {
        return private_no_cache(get_team_members(&state, &identity, Some(registry), &d, &e));
    }
    not_found()
}

// --------------------------------------------------------------------
// PUT handlers — adduser, publish, dist-tag write.
// --------------------------------------------------------------------

/// `PUT /{name}` — publish an unscoped package.
async fn put_one_segment(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    Path(name): Path<String>,
    body: axum::body::Bytes,
) -> Response {
    publish_package(&state, &identity, None, &name, body).await
}

/// `PUT /{first}/{second}` — publish a scoped package
/// (`/@scope/name`), or an unscoped package through a registry endpoint
/// (`/~<name>/<pkg>`). The `/-/package/{pkg}` shape never lands here
/// because that's at least 4 segments.
async fn put_two_segments(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    Path((first, second)): Path<(String, String)>,
    body: axum::body::Bytes,
) -> Response {
    // `PUT /~<name>/<pkg>` — publish an unscoped package through a registry.
    if let Some(registry) = tilde_registry(&first) {
        return publish_package(&state, &identity, Some(registry), &second, body).await;
    }
    if first.starts_with('@') {
        let full = format!("{first}/{second}");
        return publish_package(&state, &identity, None, &full, body).await;
    }
    not_found()
}

/// `PUT /{pkg}/-rev/{rev}` — packument update (partial unpublish).
async fn put_three_segments(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    Path((first, second, third)): Path<(String, String, String)>,
    body: axum::body::Bytes,
) -> Response {
    // `PUT /~<name>/@scope/<pkg>` — publish a scoped package through a registry.
    if let Some(registry) = tilde_registry(&first)
        && second.starts_with('@')
    {
        let full = format!("{second}/{third}");
        return publish_package(&state, &identity, Some(registry), &full, body).await;
    }
    if second == "-rev" {
        // `third` is the opaque revision token the client sent back.
        // We don't track revisions, so it's only used for routing —
        // the body is the full mutated packument.
        let _ = third;
        return update_packument(&state, &identity, None, &first, &body).await;
    }
    not_found()
}

/// 4-segment PUT:
/// * `/~<name>/{pkg}/-rev/{rev}` — packument update (partial unpublish)
///   through a registry endpoint. Scoped packages arrive percent-encoded as a
///   single `@scope%2Fname` segment, like the path-less 3-segment form.
async fn put_four_segments(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    Path((a, b, c, d)): Path<(String, String, String, String)>,
    body: axum::body::Bytes,
) -> Response {
    // `PUT /-/org/{scope}/team` — team create; config-managed, rejected.
    if a == "-" && b == "org" && d == "team" {
        return reject_team_mutation(&state, &identity, None, &c, "create a team");
    }
    if let Some(registry) = tilde_registry(&a)
        && c == "-rev"
    {
        let _ = d; // revision token is unused
        return update_packument(&state, &identity, Some(registry), &b, &body).await;
    }
    not_found()
}

/// `DELETE /{pkg}/-rev/{rev}` — remove the entire package
/// (`pnpm unpublish --force`). For scoped packages the URL is
/// `/@scope%2Fname/-rev/{rev}` and arrives as a single segment after
/// axum's percent-decoding.
async fn delete_three_segments(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    Path((first, second, third)): Path<(String, String, String)>,
) -> Response {
    if second == "-rev" {
        let _ = third;
        return delete_package(&state, &identity, None, &first).await;
    }
    not_found()
}

/// 5-segment PUT:
/// * `/-/package/{pkg}/dist-tags/{tag}` — add/update a dist-tag.
/// * `/-/team/{scope}/{team}/user` — team member add; config-managed,
///   rejected.
/// * `/~<name>/-/org/{scope}/team` — team create through a registry
///   endpoint; config-managed, rejected.
async fn put_five_segments(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    Path((a, b, c, d, e)): Path<(String, String, String, String, String)>,
    body: axum::body::Bytes,
) -> Response {
    if a == "-" && b == "package" && d == "dist-tags" {
        return set_dist_tag(&state, &identity, None, &c, &e, &body).await;
    }
    if a == "-" && b == "team" && e == "user" {
        return reject_team_mutation(&state, &identity, None, &c, "add a team member");
    }
    if let Some(registry) = tilde_registry(&a)
        && b == "-"
        && c == "org"
        && e == "team"
    {
        return reject_team_mutation(&state, &identity, Some(registry), &d, "create a team");
    }
    not_found()
}

/// 6-segment PUT:
/// * `/~<name>/-/package/{pkg}/dist-tags/{tag}` — add/update a dist-tag
///   through a registry endpoint.
/// * `/~<name>/-/team/{scope}/{team}/user` — team member add through a
///   registry endpoint; config-managed, rejected.
async fn put_six_segments(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    Path((a, b, c, d, e, f)): Path<(String, String, String, String, String, String)>,
    body: axum::body::Bytes,
) -> Response {
    if let Some(registry) = tilde_registry(&a)
        && b == "-"
    {
        if c == "package" && e == "dist-tags" {
            return set_dist_tag(&state, &identity, Some(registry), &d, &f, &body).await;
        }
        if c == "team" && f == "user" {
            return reject_team_mutation(
                &state,
                &identity,
                Some(registry),
                &d,
                "add a team member",
            );
        }
    }
    not_found()
}

/// 4-segment DELETE:
/// * `/-/team/{scope}/{team}` — team destroy; config-managed, rejected.
/// * `/~<name>/{pkg}/-rev/{rev}` — remove the entire package through a
///   registry endpoint (scoped packages arrive percent-encoded as one
///   `@scope%2Fname` segment).
async fn delete_four_segments(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    Path((a, b, c, d)): Path<(String, String, String, String)>,
) -> Response {
    if a == "-" && b == "team" {
        let _ = d; // team name — the mutation is rejected regardless
        return reject_team_mutation(&state, &identity, None, &c, "destroy a team");
    }
    if let Some(registry) = tilde_registry(&a)
        && c == "-rev"
    {
        let _ = d; // revision token is unused
        return delete_package(&state, &identity, Some(registry), &b).await;
    }
    not_found()
}

/// 5-segment DELETE:
/// * `/-/package/{pkg}/dist-tags/{tag}` — remove a dist-tag.
/// * `/-/team/{scope}/{team}/user` — team member remove; config-managed,
///   rejected.
/// * `/~<name>/-/team/{scope}/{team}` — team destroy through a registry
///   endpoint; config-managed, rejected.
/// * `/{pkg}/-/{filename}/-rev/{rev}` — remove an unscoped tarball
///   (one step of `pnpm unpublish <pkg>@<version>`).
async fn delete_five_segments(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    Path((a, b, c, d, e)): Path<(String, String, String, String, String)>,
) -> Response {
    if a == "-" && b == "package" && d == "dist-tags" {
        return remove_dist_tag(&state, &identity, None, &c, &e).await;
    }
    if a == "-" && b == "team" && e == "user" {
        return reject_team_mutation(&state, &identity, None, &c, "remove a team member");
    }
    if let Some(registry) = tilde_registry(&a)
        && b == "-"
        && c == "team"
    {
        let _ = e; // team name — the mutation is rejected regardless
        return reject_team_mutation(&state, &identity, Some(registry), &d, "destroy a team");
    }
    if b == "-" && d == "-rev" {
        let _ = e; // revision token is unused
        return delete_tarball(&state, &identity, None, &a, &c).await;
    }
    not_found()
}

/// 6-segment DELETE:
/// * `/{scope}/{name}/-/{filename}/-rev/{rev}` — remove a scoped
///   tarball. The pnpm unpublish flow gets here when the tarball URL
///   it reconstructs from the packument is the literal-slash scoped
///   form (`http://host/@scope/name/-/name-1.0.0.tgz`), so the
///   request lands here unencoded rather than as a 5-seg
///   `@scope%2Fname` URL.
/// * `/~<name>/-/package/{pkg}/dist-tags/{tag}` — remove a dist-tag
///   through a registry endpoint.
/// * `/~<name>/-/team/{scope}/{team}/user` — team member remove through a
///   registry endpoint; config-managed, rejected.
/// * `/~<name>/{pkg}/-/{filename}/-rev/{rev}` — remove an unscoped
///   tarball through a registry endpoint.
async fn delete_six_segments(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    Path((a, b, c, d, e, f)): Path<(String, String, String, String, String, String)>,
) -> Response {
    if a.starts_with('@') && c == "-" && e == "-rev" {
        let _ = f; // revision token is unused
        let full = format!("{a}/{b}");
        return delete_tarball(&state, &identity, None, &full, &d).await;
    }
    if let Some(registry) = tilde_registry(&a) {
        if b == "-" && c == "package" && e == "dist-tags" {
            return remove_dist_tag(&state, &identity, Some(registry), &d, &f).await;
        }
        if b == "-" && c == "team" && f == "user" {
            return reject_team_mutation(
                &state,
                &identity,
                Some(registry),
                &d,
                "remove a team member",
            );
        }
        if c == "-" && e == "-rev" {
            let _ = f; // revision token is unused
            return delete_tarball(&state, &identity, Some(registry), &b, &d).await;
        }
    }
    not_found()
}

/// 7-segment DELETE:
/// * `/~<name>/{scope}/{name}/-/{filename}/-rev/{rev}` — remove a scoped
///   tarball through a registry endpoint (the unencoded literal-slash form the
///   pnpm unpublish flow reconstructs from the packument's tarball URL).
async fn delete_seven_segments(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    Path((a, b, c, d, e, f, g)): Path<(String, String, String, String, String, String, String)>,
) -> Response {
    if let Some(registry) = tilde_registry(&a)
        && b.starts_with('@')
        && d == "-"
        && f == "-rev"
    {
        let _ = g; // revision token is unused
        let full = format!("{b}/{c}");
        return delete_tarball(&state, &identity, Some(registry), &full, &e).await;
    }
    not_found()
}
