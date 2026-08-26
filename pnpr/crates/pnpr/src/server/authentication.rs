use std::{
    net::{IpAddr, SocketAddr},
    sync::LazyLock,
};

use axum::{
    extract::{ConnectInfo, FromRequestParts, Request, State},
    http::{Method, request::Parts},
    middleware::Next,
    response::{IntoResponse, Response},
};

use pnpr_auth::TokenRecord;
use pnpr_error::RegistryError;
use pnpr_policy::{Identity, PackageRules};

use super::{AppState, PeerAddr, RegistrySource, single_authorization_header};

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
pub(super) struct AuthedCaller(pub(super) Identity);

impl<RouterState: Send + Sync> FromRequestParts<RouterState> for AuthedCaller {
    type Rejection = Response;

    async fn from_request_parts(
        parts: &mut Parts,
        _state: &RouterState,
    ) -> Result<Self, Self::Rejection> {
        // The middleware runs on every route, so the context is always
        // present; a miss means a wiring bug, surfaced as a 5xx.
        parts.extensions.get::<AuthedCaller>().cloned().ok_or_else(|| {
            RegistryError::Internal { reason: "authentication middleware did not run".to_string() }
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
    let method = request.method().clone();
    let peer = request.extensions().get::<ConnectInfo<PeerAddr>>().map(|info| info.0.0);

    let identity = match resolve_caller(&state, header.as_deref(), &method, peer).await {
        Ok(identity) => identity,
        Err(err) => return err.into_response(),
    };
    request.extensions_mut().insert(AuthedCaller(identity));
    next.run(request).await
}

/// Resolve the `Authorization` header to an [`Identity`], hitting the auth
/// backend exactly once. A bearer token is looked up as a full record so
/// its read-only / CIDR restrictions can be enforced here (a violation is
/// a `Forbidden` error); an unknown bearer token, a non-`Bearer` scheme
/// (e.g. legacy `Basic`), and a missing header all resolve to
/// [`Identity::Anonymous`]. `Err` is a backing-store failure, surfaced as a
/// 5xx so an outage isn't mistaken for "not authenticated".
async fn resolve_caller(
    state: &AppState,
    header: Option<&str>,
    method: &Method,
    peer: Option<SocketAddr>,
) -> Result<Identity, RegistryError> {
    if let Some(raw_token) = header.and_then(bearer_credentials) {
        let Some(record) = state.inner.auth.tokens.lookup_record(raw_token).await? else {
            return Ok(Identity::Anonymous);
        };
        check_token_restrictions(&record, method, peer)?;
        return Ok(Identity::user(record.username));
    }
    // Anything that is not a bearer token — Basic, another scheme, or no
    // credentials — carries no request identity. Going through `identify`
    // here would re-run the bearer lookup and bypass the restriction checks
    // above, so resolve straight to anonymous.
    Ok(Identity::Anonymous)
}

/// Enforce a bearer token's own restrictions. A read-only token may not
/// drive a mutating request; a CIDR-pinned token may only be used from a
/// whitelisted peer (and is refused when the peer address is unavailable,
/// so the check fails closed).
fn check_token_restrictions(
    record: &TokenRecord,
    method: &Method,
    peer: Option<SocketAddr>,
) -> Result<(), RegistryError> {
    if record.readonly && is_write_method(method) {
        return Err(RegistryError::Forbidden {
            user: record.username.clone(),
            action: "write with",
            resource: "a read-only token".to_string(),
        });
    }
    if !record.cidr_whitelist.is_empty() {
        // The peer address comes from the accepted socket (`ConnectInfo`),
        // never a client-supplied forwarding header.
        let allowed = peer.is_some_and(|addr| cidr_whitelist_allows(&record.cidr_whitelist, addr));
        if !allowed {
            return Err(RegistryError::Forbidden {
                user: record.username.clone(),
                action: "use",
                resource: "this token from your network address".to_string(),
            });
        }
    }
    Ok(())
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
            state.inner.config.hosted.get(name).map(|hosted| &hosted.rules)
        }
        RegistrySource::Upstream(name) => {
            state.inner.config.upstreams.get(name).map(|upstream| &upstream.rules)
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
    let effective = source_rules(state, source).for_package(package);
    let list = match action {
        Action::Access => effective.access,
        Action::Publish => effective.publish,
        Action::Unpublish => effective.unpublish,
    };
    if list.allows(identity) {
        return Ok(());
    }
    // Denied: an anonymous caller gets a chance to authenticate (401);
    // an authenticated caller simply isn't in the allowed set (403).
    match identity {
        Identity::Anonymous => {
            Err(RegistryError::Unauthenticated { resource: format!("package {package:?}") })
        }
        Identity::User { username, .. } => Err(RegistryError::Forbidden {
            user: username.clone(),
            action: action.label(),
            resource: format!("package {package:?}"),
        }),
    }
}

/// The raw credentials of an `Authorization: Bearer <token>` header, or
/// `None` for any other scheme. The scheme is matched case-insensitively,
/// matching [`pnpr_auth::identify`].
pub(super) fn bearer_credentials(header_value: &str) -> Option<&str> {
    let (scheme, credentials) = header_value.trim().split_once(' ')?;
    scheme.eq_ignore_ascii_case("Bearer").then(|| credentials.trim())
}

/// Whether `method` mutates registry state. Every write surface (publish,
/// unpublish, dist-tag add/remove, adduser, logout, token revoke) is a
/// PUT or DELETE; reads and the resolver POSTs are not. A read-only token
/// is confined to the non-mutating methods.
pub(super) fn is_write_method(method: &Method) -> bool {
    matches!(*method, Method::PUT | Method::DELETE | Method::PATCH)
}

/// Whether `peer` falls inside any range of a token's CIDR whitelist. An
/// IPv4-mapped IPv6 peer is normalized to its IPv4 form first, so a
/// dual-stack listener still matches plain IPv4 ranges.
pub(super) fn cidr_whitelist_allows(whitelist: &[String], peer: SocketAddr) -> bool {
    let peer = canonical_ip(peer.ip());
    whitelist.iter().any(|entry| cidr_contains(entry.trim(), peer))
}

pub(super) fn canonical_ip(addr: IpAddr) -> IpAddr {
    match addr {
        IpAddr::V6(v6) => v6.to_ipv4_mapped().map_or(IpAddr::V6(v6), IpAddr::V4),
        v4 @ IpAddr::V4(_) => v4,
    }
}

/// Whether `peer` is inside one `addr/prefix` (or bare `addr`) whitelist
/// entry. A bare address matches only itself; a malformed entry (bad
/// address, or a non-numeric / out-of-range prefix) matches nothing, so
/// the restriction fails closed rather than open.
pub(super) fn cidr_contains(entry: &str, peer: IpAddr) -> bool {
    let (net, prefix) = match entry.split_once('/') {
        Some((net, prefix)) => (net.trim(), Some(prefix.trim())),
        None => (entry, None),
    };
    let Ok(net) = net.parse::<IpAddr>() else {
        return false;
    };
    match (net, peer) {
        (IpAddr::V4(net), IpAddr::V4(peer)) => {
            let Some(bits) = parse_prefix(prefix, 32) else {
                return false;
            };
            let mask = ipv4_mask(bits);
            (u32::from(net) & mask) == (u32::from(peer) & mask)
        }
        (IpAddr::V6(net), IpAddr::V6(peer)) => {
            let Some(bits) = parse_prefix(prefix, 128) else {
                return false;
            };
            let mask = ipv6_mask(bits);
            (u128::from(net) & mask) == (u128::from(peer) & mask)
        }
        // Different address families never match.
        _ => false,
    }
}

/// Parse a CIDR prefix length, defaulting to a full-width match (an exact
/// host) when the entry carried no `/prefix`. `None` for a non-numeric or
/// too-large value.
fn parse_prefix(prefix: Option<&str>, max_bits: u8) -> Option<u8> {
    match prefix {
        None => Some(max_bits),
        Some(prefix) => {
            let bits: u8 = prefix.parse().ok()?;
            (bits <= max_bits).then_some(bits)
        }
    }
}

fn ipv4_mask(prefix: u8) -> u32 {
    if prefix == 0 { 0 } else { u32::MAX << (32 - prefix) }
}

fn ipv6_mask(prefix: u8) -> u128 {
    if prefix == 0 { 0 } else { u128::MAX << (128 - prefix) }
}
