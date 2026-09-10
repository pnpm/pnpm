use super::{
    AppState, Config, HeaderMap, Identity, Next, Registry, RegistryError, Request, Response, State,
    Upstream, authentication, header, identify,
};
use axum::response::IntoResponse;

/// Resolve the upstream behind an authorized `/~<name>/` endpoint request.
///
/// Fails closed: an upstream that does not exist or carries no `access:` policy
/// is a `404` (it is not a private-route endpoint), and a caller the policy
/// does not admit is a `403`. Returns the [`Upstream`] to fetch *through* —
/// `/~<name>/` requests never read or write the shared proxy mirror, so a
/// private upstream's packuments and tarballs can never leak across the public
/// path or another upstream.
pub(super) fn authorized_upstream<'a>(
    state: &'a AppState,
    identity: &Identity,
    upstream: &str,
) -> Result<&'a Upstream, RegistryError> {
    let Some(config) = state.inner.config.upstreams.get(upstream) else {
        return Err(RegistryError::NotFound);
    };
    // A private upstream registry gates by its `access:` list; a public registry
    // (no access) is reachable by anyone at its `/~<name>/` URL, its upstream
    // credential (if any) staying server-side either way.
    if let Some(access) = config.access.as_ref()
        && !access.allows(identity)
    {
        let user = require_caller(identity, "upstream access")
            .unwrap_or_else(|_| "<anonymous>".to_string());
        return Err(RegistryError::Forbidden {
            user,
            action: "access",
            resource: format!("upstream {upstream:?}"),
        });
    }
    state.inner.upstreams.get(upstream).ok_or_else(|| RegistryError::NotFound)
}

pub(super) fn authorized_revision_upstream<'a>(
    state: &'a AppState,
    identity: &Identity,
    registry: &str,
) -> Result<&'a Upstream, RegistryError> {
    if !matches!(state.inner.config.registries.get(registry), Some(Registry::Upstream { .. })) {
        return Err(RegistryError::NotFound);
    }
    let Some(config) = state.inner.config.upstreams.get(registry) else {
        return Err(RegistryError::NotFound);
    };
    if config.rules.refines_access() {
        return Err(RegistryError::NotFound);
    }
    authorized_upstream(state, identity, registry)
}

pub(super) fn revision_registry_is_private(state: &AppState, registry: &str) -> bool {
    state.inner.config.upstreams.get(registry).is_some_and(|config| config.access.is_some())
}

pub(super) fn revision_source_registry<'a>(
    state: &'a AppState,
    addressed_registry: &str,
    source: &str,
) -> Option<&'a str> {
    if addressed_registry != source {
        return None;
    }
    let config = state.inner.config.upstreams.get(source)?;
    (!config.rules.refines_access()).then_some(config.url.as_str())
}

/// The disposable cache namespace for an upstream registry's `/~<name>/` route —
/// the entry precomputed in [`AppInner::upstream_cache_namespaces`](super::AppInner::upstream_cache_namespaces), falling back
/// to a fresh computation only for a name outside [`Config::upstreams`] (which
/// the registry dispatch never produces).
pub(super) fn upstream_cache_namespace(state: &AppState, upstream: &str) -> String {
    state
        .inner
        .upstream_cache_namespaces
        .get(upstream)
        .cloned()
        .unwrap_or_else(|| compute_upstream_cache_namespace(&state.inner.config, upstream))
}

/// Compute an upstream registry's disposable cache namespace, so its packuments
/// and tarballs never collide with another registry's.
///
/// Both shapes fold in the registry's upstream **URL**: the cache is a mirror of
/// one declared origin, so repointing a registry's `url:` moves to a fresh
/// namespace and bytes fetched from the previous origin can never answer for
/// the new one. The cache-first warm tarball path depends on this — it serves
/// a cached entry without re-binding it against the current packument.
///
/// A **private** registry — any that declares `access:` (so it is not `public`; the
/// config loader forbids a public registry from carrying any credential) — is
/// namespaced by an HMAC over `(registry, url, credential)` keyed with
/// the server secret: the on-disk path leaks neither the registry name nor the
/// credential, and a credential rotation moves to a fresh namespace. Keying on
/// the declared visibility rather than on the presence of an `Authorization`
/// header keeps a registry whose credential rides a *custom* header (or which
/// gates access without an upstream credential) out of the guessable public
/// namespace. A **public** registry has nothing private to protect and its content
/// is integrity-verified, so it uses a *stable* namespace
/// (`~public/<digest-of-registry-name-and-url>`) that is shared across process
/// restarts.
pub(super) fn compute_upstream_cache_namespace(config: &Config, upstream: &str) -> String {
    let url =
        config.upstreams.get(upstream).map_or("", |upstream_config| upstream_config.url.as_str());
    if let Some(upstream_config) = config.upstreams.get(upstream)
        && upstream_config.access.is_some()
    {
        // The credential epoch covers the origin URL and every header the
        // upstream attaches upstream, not just `Authorization`, so repointing
        // the URL or rotating a credential carried in a custom header moves
        // the private cache to a fresh namespace. The NUL separator keeps
        // `(url, headers)` pairs unambiguous — a URL cannot contain NUL.
        let epoch = pnpr_route::credential_digest(&format!(
            "{url}\0{}",
            pnpr_route::headers_credential_digest(&upstream_config.headers),
        ));
        let digest =
            pnpr_route::upstream_cache_digest(upstream, epoch, &config.resolution_cache_secret);
        return format!("~upstreams/{digest}");
    }
    // Public registry: a stable, secret-free namespace keyed by the registry name
    // and its origin URL (hashed so a path-unsafe value can't escape the
    // cache root).
    format!("~public/{}", pnpr_route::credential_digest(&format!("{upstream}\0{url}")))
}

/// Require that an endpoint's caller is authenticated, returning their
/// username or the 401 error to send back. The identity was already
/// resolved by the [`authenticate`](super::authentication::authenticate) middleware (which is also where an
/// auth-backend outage surfaces as a 5xx), so this is a pure check.
/// `resource` names what the 401 is about.
pub(super) fn require_caller(identity: &Identity, resource: &str) -> Result<String, RegistryError> {
    match identity {
        Identity::User { username, .. } => Ok(username.clone()),
        Identity::Anonymous => {
            Err(RegistryError::Unauthenticated { resource: resource.to_string() })
        }
    }
}

pub(super) async fn caller_username(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<Option<String>, RegistryError> {
    let authorization = single_authorization_header(headers)?;
    if let Some(raw) = authorization.and_then(authentication::bearer_credentials)
        && let Some(username) = state.inner.oidc.session(raw)?
    {
        return Ok(Some(username));
    }
    identify(authorization, state.inner.auth.tokens.as_ref()).await
}

pub(super) async fn require_resolver_caller(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    require_protocol_caller(&state, request, next, "dependency resolution").await
}

pub(super) async fn require_artifact_caller(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    require_protocol_caller(&state, request, next, "shared artifacts").await
}

pub(super) async fn require_pipeline_caller(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    require_protocol_caller(&state, request, next, "pipeline runs").await
}

pub(super) async fn require_protocol_caller(
    state: &AppState,
    request: Request,
    next: Next,
    resource: &str,
) -> Response {
    match caller_username(state, request.headers()).await {
        Ok(Some(_username)) => next.run(request).await,
        Ok(None) => {
            RegistryError::Unauthenticated { resource: resource.to_string() }.into_response()
        }
        Err(error) => error.into_response(),
    }
}

pub(super) fn single_authorization_header(
    headers: &HeaderMap,
) -> Result<Option<&str>, RegistryError> {
    let mut values = headers.get_all(header::AUTHORIZATION).iter();
    let Some(value) = values.next() else {
        return Ok(None);
    };
    if values.next().is_some() {
        return Err(RegistryError::BadRequest {
            reason: "multiple Authorization headers are not allowed".to_string(),
        });
    }
    value.to_str().map(Some).map_err(|_| RegistryError::BadRequest {
        reason: "Authorization header is not valid text".to_string(),
    })
}
