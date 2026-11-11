pub(super) use accounts::require_active_account;
pub(super) use credentials::token_credentials;
pub(super) use package_access::{
    Ownership, authorize, authorize_against_packument, authorize_continuation, authorize_package,
    authorize_with_ownership,
};

mod accounts;
mod credentials;
mod package_access;

use accounts::{account_identity, active_account_name};

use std::net::IpAddr;

use axum::{
    extract::{ConnectInfo, FromRequestParts, Request, State},
    http::{HeaderMap, Method, StatusCode, header, request::Parts},
    middleware::Next,
    response::{IntoResponse, Response},
};

use pnpr_auth::TokenRecord;
use pnpr_error::RegistryError;
use pnpr_policy::Identity;

use super::{
    AppState, PeerAddr, RegistrySource, oidc::WorkloadScope,
    restrictions::check_token_restrictions, single_authorization_header,
};

#[derive(Clone)]
pub(super) struct InvalidToken;

/// A password login authenticates independently of the Authorization header.
#[derive(Clone)]
pub(super) struct PasswordAuthentication;

/// Whether `headers` carry `Authorization: Bearer <expected>`, for an
/// endpoint authenticated by a configured secret instead of a user token.
/// Comparing digests keeps the comparison time independent of how much of
/// the secret a guess gets right.
pub(super) fn carries_bearer_secret(headers: &HeaderMap, expected: &str) -> bool {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .is_some_and(|sent| {
            super::ecosystem::sha256_hex(expected.as_bytes())
                == super::ecosystem::sha256_hex(sent.trim().as_bytes())
        })
}

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
    if let Err(error) = check_request_schema(&state, request.uri().path()).await {
        return error.into_response();
    }
    super::managed_state::refresh_managed_state(&state).await;
    let method = request.method().clone();
    let path = request.uri().path().to_owned();
    let client_ip = resolve_and_attach_client_ip(&mut request, &state);

    if let Some(raw) = header.as_deref().and_then(token_credentials) {
        match bearer_token_identity(&state, &raw, &method, &path, client_ip).await {
            Ok(Some(identity)) => {
                if let Err(err) = refuse_deprovisioned(&state, &identity) {
                    return token_failure_response(err.into_response(), header.as_deref());
                }
                let caller = AuthedCaller(identity);
                record_user_on_span(&caller.0);
                request.extensions_mut().insert(caller.clone());
                let mut response = next.run(request).await;
                response.extensions_mut().insert(caller);
                return token_failure_response(response, header.as_deref());
            }
            Ok(None) => {}
            Err(response) => return token_failure_response(response, header.as_deref()),
        }
    }

    let resolved = resolve_caller(&state, header.as_deref(), &method, &path, client_ip).await;
    let caller = match resolved.and_then(|resolved| {
        refuse_deprovisioned(&state, &resolved.0)?;
        Ok(resolved)
    }) {
        Ok(resolved) => attach_caller(&mut request, resolved),
        Err(err) => return token_failure_response(err.into_response(), header.as_deref()),
    };
    let mut response = next.run(request).await;
    response.extensions_mut().insert(caller);
    token_failure_response(response, header.as_deref())
}

async fn check_request_schema(state: &AppState, path: &str) -> Result<(), RegistryError> {
    if path == "/-/live"
        || path
            .strip_prefix("/~")
            .and_then(|path| path.split_once('/'))
            .is_some_and(|(registry, tail)| !registry.is_empty() && tail == "-/live")
    {
        return Ok(());
    }
    state.inner.storage.sql_metadata().check_schema().await
}

fn token_failure_response(mut response: Response, header: Option<&str>) -> Response {
    if response.status() == StatusCode::UNAUTHORIZED
        && response
            .extensions()
            .get::<PasswordAuthentication>()
            .is_none()
        && response
            .extensions()
            .get::<AuthedCaller>()
            .is_none_or(|caller| matches!(caller.0, Identity::Anonymous))
        && header.and_then(token_credentials).is_some()
    {
        response.extensions_mut().insert(InvalidToken);
    }
    response
}

/// Record the resolved caller, and a workload's scope, on `request`.
fn attach_caller(
    request: &mut Request,
    (identity, workload): (Identity, Option<WorkloadScope>),
) -> AuthedCaller {
    let caller = AuthedCaller(identity);
    record_user_on_span(&caller.0);
    request.extensions_mut().insert(caller.clone());
    if let Some(workload) = workload {
        request.extensions_mut().insert(workload);
    }
    caller
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
        Identity::User { id, .. } if super::scim::is_deprovisioned(state, id)? => {
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
            Ok(Some(
                account_identity(state, &record.user_id).await
                    .map_err(IntoResponse::into_response)?,
            ))
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
    let now = pnpr_auth::unix_millis();
    if record.expires_at.is_some_and(|exp| exp <= now) {
        return Ok(None);
    }
    let owned = auth.users.account_name(&record.user_id).await?.is_some();
    Ok(owned.then_some(record))
}

/// Resolve the `Authorization` header to an [`Identity`], hitting the auth
/// backend at most once. Browser sessions and workload credentials check
/// whether their account still exists. Invalid OIDC credentials are rejected even on
/// public routes.
/// A pnpr token — however the client carried it, see
/// [`token_credentials`] — is looked up as a full record so its read-only /
/// CIDR restrictions can be enforced here (a violation is a `Forbidden`
/// error); an unknown token, any other credential shape, and a missing
/// header all resolve to [`Identity::Anonymous`]. `Err` is a backing-store
/// failure, surfaced as a 5xx so an outage isn't mistaken for "not
/// authenticated". Workload credentials also give the [`WorkloadScope`] a
/// batch commit checks its entries against.
async fn resolve_caller(
    state: &AppState,
    header: Option<&str>,
    method: &Method,
    path: &str,
    client_ip: Option<IpAddr>,
) -> Result<(Identity, Option<WorkloadScope>), RegistryError> {
    if let Some(raw_token) = header.and_then(token_credentials) {
        if let Some(user) = state.inner.identity.oidc.session(&raw_token)? {
            let name = active_account_name(state, &user.user_id).await?;
            return Ok((
                super::oidc_groups::session_identity(&state.inner.config, user, name),
                None,
            ));
        }
        if let Some(jwt) = raw_token.strip_prefix("pnpr_workload_") {
            let workload = state.inner.identity.oidc
                .workload(jwt)
                .await?
                .ok_or_else(|| RegistryError::Unauthenticated {
                    resource: "OIDC workload credentials".to_string(),
                })?;
            super::oidc::check_workload_request(&workload, method, path)?;
            let scope = WorkloadScope::new(&state.inner.config, &workload);
            return Ok((account_identity(state, &workload.identity.user_id).await?, Some(scope)));
        }
        if let Some(record) = state.inner.identity.auth.tokens.lookup_record(&raw_token).await? {
            check_token_restrictions(&record, method, path, client_ip)?;
            state.inner.identity.last_used.record_raw(&raw_token, record.last_used_at);
            let identity = match record.owner_name().map(str::to_string) {
                Some(name) => Identity::account(record.user_id, name),
                None => account_identity(state, &record.user_id).await?,
            };
            return Ok((identity, None));
        }
    }
    // Anything that is not a pnpr token — a user:password Basic pair, another
    // scheme, or no credentials — carries no request identity. Going through
    // `identify` here would re-run the token lookup and bypass the
    // restriction checks above, so resolve straight to anonymous.
    Ok((Identity::Anonymous, None))
}
