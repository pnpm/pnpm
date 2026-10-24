use std::{
    net::IpAddr,
    sync::LazyLock,
    time::{SystemTime, UNIX_EPOCH},
};

use axum::{
    extract::{ConnectInfo, FromRequestParts, Request, State},
    http::{Method, request::Parts},
    middleware::Next,
    response::{IntoResponse, Response},
};

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use pnpr_auth::TokenRecord;
use pnpr_error::RegistryError;
use pnpr_policy::{Identity, PackageRules};

use super::{
    AppState, PeerAddr, RegistrySource, restrictions::check_token_restrictions,
    single_authorization_header,
};

/// What the caller is trying to do with a package. Drives which
/// rule from the access policy applies.
#[derive(Debug, Clone, Copy)]
pub(super) enum Action {
    Access,
    Publish,
    Unpublish,
}

impl Action {
    fn label(self) -> &'static str {
        match self {
            Action::Access => "access",
            Action::Publish => "publish",
            Action::Unpublish => "unpublish",
        }
    }
}

/// The caller resolved once by the [`authenticate`] middleware and stored
/// in request extensions. Every registry handler that needs to know who is
/// calling reads it back through this extractor rather than re-inspecting
/// the `Authorization` header — so a request hits the auth backend exactly
/// once, and the identity a handler sees is the same one the restriction
/// gate already approved (no second lookup, no policy/identity race).
#[derive(Clone)]
pub(crate) struct AuthedCaller(pub(super) Identity);

impl<RouterState: Send + Sync> FromRequestParts<RouterState> for AuthedCaller {
    type Rejection = Response;

    async fn from_request_parts(
        parts: &mut Parts,
        _state: &RouterState,
    ) -> Result<Self, Self::Rejection> {
        // The middleware runs on every route, so the context is always
        // present; a miss means a wiring bug, surfaced as a 5xx.
        parts.extensions
            .get::<AuthedCaller>()
            .cloned()
            .ok_or_else(|| {
                RegistryError::Internal {
                    reason: "authentication middleware did not run".to_string(),
                }
                .into_response()
            })
    }
}

/// Authenticate every request once, up front, and stash the resolved
/// [`Identity`] in request extensions for the handlers (via
/// [`AuthedCaller`]).
///
/// This is also where bearer-token restrictions are enforced — ahead of
/// every route handler, so a restricted token is rejected before a write
/// handler buffers its (up to 100 MiB) request body. npm bearer tokens can
/// be marked read-only or pinned to a set of CIDR ranges; pnpr persists
/// both and surfaces them on `npm token list`, so it must enforce them too
/// — otherwise a token the operator restricted could still publish, or be
/// used from any network. Basic-auth and anonymous requests carry no
/// restriction and are still subject to the per-package access policy in
/// the handlers; an unknown or revoked bearer token resolves to anonymous.
pub(super) async fn authenticate(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Response {
    // Copy what resolution needs out of the request before mutating its
    // extensions below — the header and method borrows can't outlive the
    // `extensions_mut` call.
    let header = match single_authorization_header(request.headers()) {
        Ok(header) => header.map(str::to_owned),
        Err(err) => return err.into_response(),
    };
    super::managed_state::refresh_managed_state(&state).await;
    let method = request.method().clone();
    let path = request.uri().path().to_owned();
    let client_ip = resolve_and_attach_client_ip(&mut request, &state);

    if let Some(raw) = header.as_deref().and_then(token_credentials) {
        match bearer_token_identity(&state, &raw, &method, &path, client_ip).await {
            Ok(Some(identity)) => {
                if let Err(err) = refuse_deprovisioned(&state, &identity) {
                    return err.into_response();
                }
                let caller = AuthedCaller(identity);
                record_user_on_span(&caller.0);
                request.extensions_mut().insert(caller.clone());
                let mut response = next.run(request).await;
                response.extensions_mut().insert(caller);
                return response;
            }
            Ok(None) => {}
            Err(response) => return response,
        }
    }

    let resolved = resolve_caller(&state, header.as_deref(), &method, &path, client_ip).await;
    let identity = match resolved.and_then(|identity| {
        refuse_deprovisioned(&state, &identity)?;
        Ok(identity)
    }) {
        Ok(identity) => identity,
        Err(err) => return err.into_response(),
    };
    let caller = AuthedCaller(identity);
    record_user_on_span(&caller.0);
    request.extensions_mut().insert(caller.clone());
    let mut response = next.run(request).await;
    response.extensions_mut().insert(caller);
    response
}

fn record_user_on_span(identity: &Identity) {
    if let Some(username) = identity.username() {
        tracing::Span::current().record("user", tracing::field::display(username));
    } else {
        tracing::Span::current().record("user", "-");
    }
}

fn resolve_and_attach_client_ip(request: &mut Request, state: &AppState) -> Option<IpAddr> {
    let peer = request
        .extensions()
        .get::<ConnectInfo<PeerAddr>>()
        .map(|info| info.0.0.ip());
    let client_ip = super::resolve_client_ip(
        peer,
        request.headers(),
        &state.inner.config.http.trusted_proxies,
        state.inner.config.http.trusted_proxy_header,
    );
    if let Some(ip) = client_ip {
        request
            .extensions_mut()
            .insert(super::ClientIp(ip));
    }
    client_ip
}

/// Refuse the credentials of a user a SCIM client deprovisioned. Its stored
/// tokens are already revoked, but a browser session on another replica
/// lives until that replica reloads the directory.
pub(super) fn refuse_deprovisioned(
    state: &AppState,
    identity: &Identity,
) -> Result<(), RegistryError> {
    match identity {
        Identity::User { username, .. } if super::scim::is_deprovisioned(state, username)? => {
            Err(RegistryError::Unauthenticated { resource: "a deprovisioned account".to_string() })
        }
        _ => Ok(()),
    }
}

/// The identity an OCI bearer token carries, or `None` when the credential is
/// not one of pnpr's own bearer tokens and the ordinary backend lookup should
/// decide instead.
async fn bearer_token_identity(
    state: &AppState,
    raw: &str,
    method: &Method,
    path: &str,
    client_ip: Option<IpAddr>,
) -> Result<Option<Identity>, Response> {
    let claims = match super::oci::tokens::decode(state, raw) {
        Ok(Some(claims)) => claims,
        Ok(None) => return Ok(None),
        Err(RegistryError::Unauthenticated { .. }) => {
            return Err(super::oci::tokens::rejected(state, path, method));
        }
        Err(err) => return Err(err.into_response()),
    };
    if !claims.permits(path, method) {
        return Err(super::oci::tokens::rejected(state, path, method));
    }
    let Some(parent) = claims.parent.as_ref() else {
        return Ok(Some(Identity::Anonymous));
    };
    match owned_parent(state, parent).await {
        Ok(Some(record)) => {
            check_token_restrictions(&record, method, path, client_ip)
                .map_err(axum::response::IntoResponse::into_response)?;
            state.inner.identity.last_used.record(parent, record.last_used_at);
            Ok(Some(Identity::user(record.username)))
        }
        Ok(None) => Err(super::oci::tokens::rejected(state, path, method)),
        Err(err) => Err(err.into_response()),
    }
}

/// The token an OCI bearer token was issued from, while its owner is still an
/// account, as [`pnpr_auth::TokenBackend::lookup_record`] requires of a token
/// sent directly.
async fn owned_parent(state: &AppState, key: &str) -> Result<Option<TokenRecord>, RegistryError> {
    let auth = &state.inner.identity.auth;
    let Some(record) = auth.tokens.find_by_key(key).await? else {
        return Ok(None);
    };
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    if record.expires_at.is_some_and(|exp| exp <= now) {
        return Ok(None);
    }
    let owned = auth.users.password_hash(&record.username).await?.is_some();
    Ok(owned.then_some(record))
}

/// Resolve the `Authorization` header to an [`Identity`], hitting the auth
/// backend at most once. Browser sessions and prefixed workload credentials
/// skip the persistent backend. Invalid OIDC
/// credentials are rejected even on public routes.
/// A pnpr token — however the client carried it, see
/// [`token_credentials`] — is looked up as a full record so its read-only /
/// CIDR restrictions can be enforced here (a violation is a `Forbidden`
/// error); an unknown token, any other credential shape, and a missing
/// header all resolve to [`Identity::Anonymous`]. `Err` is a backing-store
/// failure, surfaced as a 5xx so an outage isn't mistaken for "not
/// authenticated".
async fn resolve_caller(
    state: &AppState,
    header: Option<&str>,
    method: &Method,
    path: &str,
    client_ip: Option<IpAddr>,
) -> Result<Identity, RegistryError> {
    if let Some(raw_token) = header.and_then(token_credentials) {
        if let Some(user) = state.inner.identity.oidc.session(&raw_token)? {
            return Ok(super::oidc_groups::session_identity(&state.inner.config, user));
        }
        if let Some(jwt) = raw_token.strip_prefix("pnpr_workload_") {
            let workload = state.inner.identity.oidc
                .workload(jwt)
                .await?
                .ok_or_else(|| RegistryError::Unauthenticated {
                    resource: "OIDC workload credentials".to_string(),
                })?;
            super::oidc::check_workload_request(&workload, method, path)?;
            return Ok(Identity::user(workload.identity.username));
        }
        if let Some(record) = state.inner.identity.auth.tokens.lookup_record(&raw_token).await? {
            check_token_restrictions(&record, method, path, client_ip)?;
            state.inner.identity.last_used.record_raw(&raw_token, record.last_used_at);
            return Ok(Identity::user(record.username));
        }
    }
    // Anything that is not a pnpr token — a user:password Basic pair, another
    // scheme, or no credentials — carries no request identity. Going through
    // `identify` here would re-run the token lookup and bypass the
    // restriction checks above, so resolve straight to anonymous.
    Ok(Identity::Anonymous)
}

/// The pnpr token an `Authorization` header carries, in any of the shapes
/// the supported clients send: `Bearer <token>` (npm, pnpm), the bare
/// `<token>` with no scheme (`cargo`, which sends registry tokens raw), or
/// `Basic` with the token as the password (`twine` and `pip` pair it with
/// pypi.org's `__token__` pseudo-user, `docker` and `podman` with whatever
/// name the user typed at login).
///
/// The password is the whole credential: the `Basic` username is not checked
/// against the token's owner, so a caller authenticates as whoever the token
/// belongs to. That is what every registry taking a personal access token as
/// a password does, and what its clients expect.
pub(super) fn token_credentials(header_value: &str) -> Option<String> {
    let value = header_value.trim();
    if let Some(bearer) = bearer_credentials(value) {
        return Some(bearer.to_string());
    }
    let Some((scheme, credentials)) = value.split_once(' ') else {
        return (!value.is_empty()).then(|| value.to_string());
    };
    if !scheme.eq_ignore_ascii_case("Basic") {
        return None;
    }
    let decoded = BASE64_STANDARD.decode(credentials.trim()).ok()?;
    let decoded = String::from_utf8(decoded).ok()?;
    let (_, password) = decoded.split_once(':')?;
    (!password.is_empty()).then(|| password.to_string())
}

/// The `packages:` rules of the concrete registry a request resolved to.
/// Authorization is entirely registry-scoped — there is no global,
/// name-keyed ACL — so every check consults the one registry that serves
/// the package. The fallback (safe defaults: reads open, publishes need
/// auth, destructive writes denied) only fires for a programmatically
/// built config whose serving tables miss the graph entry.
fn source_rules<'a>(state: &'a AppState, source: &RegistrySource) -> &'a PackageRules {
    static SAFE_DEFAULTS: LazyLock<PackageRules> = LazyLock::new(PackageRules::default);
    match source {
        RegistrySource::Hosted(name) => {
            state.inner.config.routing.hosted.get(name).map(|hosted| &hosted.rules)
        }
        RegistrySource::Upstream(name) => {
            state.inner.config.routing.upstreams.get(name).map(|upstream| &upstream.rules)
        }
        RegistrySource::Unclaimed | RegistrySource::NotFound => None,
    }
    .unwrap_or(&SAFE_DEFAULTS)
}

/// Check an already-resolved `identity` against the resolved source
/// registry's per-package rule (the most specific `packages:` entry, its
/// omitted fields falling back to the registry defaults). Returns `Ok(())`
/// when the call is allowed; otherwise the appropriate `Unauthenticated` /
/// `Forbidden` error. The identity is resolved once by [`authenticate`], so
/// every handler — including the search endpoint that filters many packages —
/// authorizes synchronously against it.
pub(super) fn authorize(
    state: &AppState,
    identity: &Identity,
    source: &RegistrySource,
    package: &str,
    action: Action,
) -> Result<(), RegistryError> {
    let rules = source_rules(state, source).snapshot();
    let effective = rules.for_package(package);
    let list = match action {
        Action::Access => effective.access,
        Action::Publish => effective.publish,
        Action::Unpublish => effective.unpublish,
    };
    if list.allows(identity) {
        return Ok(());
    }
    // Denied: an anonymous caller gets a chance to authenticate (401);
    // an authenticated caller isn't in the allowed set (403).
    match identity {
        Identity::Anonymous => {
            Err(RegistryError::Unauthenticated { resource: format!("package {package:?}") })
        }
        Identity::User { username, .. } => Err(RegistryError::Forbidden {
            user: username.clone(),
            action: action.label(),
            resource: format!("package {package:?}"),
        }),
        _ => Err(RegistryError::Unauthenticated { resource: format!("package {package:?}") }),
    }
}

pub(super) async fn is_package_owner(
    state: &AppState,
    identity: &Identity,
    package: &str,
    org: Option<&str>,
    action: Action,
) -> Result<bool, RegistryError> {
    let Some(username) = identity.username() else {
        return Ok(false);
    };
    if state.inner.config.identity.admins.contains(username) {
        return Ok(true);
    }
    let Ok(canonical) =
        pnpr_package_name::CanonicalPackageName::parse(package, pnpr_package_name::Ecosystem::Npm)
    else {
        return Ok(false);
    };
    let storage = super::hosted_storage(state, org);
    if let Some(bytes) = storage.read_hosted_document(&canonical).await? {
        if let Ok(val) = serde_json::from_slice::<serde_json::Value>(&bytes)
            && let Some(maintainers) = val.get("maintainers").and_then(serde_json::Value::as_array)
        {
            let is_maintainer = maintainers
                .iter()
                .any(|entry| {
                    entry.get("name").and_then(serde_json::Value::as_str) == Some(username)
                });
            return Ok(is_maintainer);
        }
        return Ok(false);
    }
    if let Some(tombstone) = storage.read_tombstone(&canonical).await? {
        return Ok(tombstone.maintainers
            .iter()
            .any(|maintainer| maintainer == username));
    }
    // Brand new package: initial publish claims the package; cannot unpublish nonexistent package
    Ok(matches!(action, Action::Publish))
}

pub(super) async fn authorize_package(
    state: &AppState,
    identity: &Identity,
    source: &RegistrySource,
    package: &str,
    action: Action,
    org: Option<&str>,
) -> Result<(), RegistryError> {
    let rules = source_rules(state, source).snapshot();
    let effective = rules.for_package(package);
    let list = match action {
        Action::Access => &effective.access,
        Action::Publish => &effective.publish,
        Action::Unpublish => &effective.unpublish,
    };
    if list.allows(identity) {
        return Ok(());
    }
    if list.references_owners() && identity.is_authenticated() {
        let is_owner = is_package_owner(state, identity, package, org, action).await?;
        if list.allows_with_context(identity, is_owner) {
            return Ok(());
        }
    }
    match identity {
        Identity::Anonymous => {
            Err(RegistryError::Unauthenticated { resource: format!("package {package:?}") })
        }
        Identity::User { username, .. } => Err(RegistryError::Forbidden {
            user: username.clone(),
            action: action.label(),
            resource: format!("package {package:?}"),
        }),
        _ => Err(RegistryError::Unauthenticated { resource: format!("package {package:?}") }),
    }
}

/// The raw credentials of an `Authorization: Bearer <token>` header, or
/// `None` for any other scheme. The scheme is matched case-insensitively,
/// matching [`pnpr_auth::identify`].
pub(super) fn bearer_credentials(header_value: &str) -> Option<&str> {
    let (scheme, credentials) = header_value.trim().split_once(' ')?;
    scheme.eq_ignore_ascii_case("Bearer").then(|| credentials.trim())
}
