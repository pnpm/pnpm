use std::net::IpAddr;

use axum::http::Method;
use pnpr_auth::TokenRecord;
use pnpr_error::RegistryError;
use pnpr_registry::Ecosystem;

/// Enforce a bearer token's own restrictions. A read-only token may not
/// drive a mutating request; a CIDR-pinned token may only be used from a
/// whitelisted peer (and is refused when the peer address is unavailable,
/// so the check fails closed).
pub(super) fn check_token_restrictions(
    record: &TokenRecord,
    method: &Method,
    path: &str,
    client_ip: Option<IpAddr>,
) -> Result<(), RegistryError> {
    if record.readonly && is_write_request(method, path) {
        return Err(RegistryError::Forbidden {
            user: record.username.clone(),
            action: "write with",
            resource: "a read-only token".to_string(),
        });
    }
    if !record.cidr_whitelist.is_empty() {
        let allowed = client_ip.is_some_and(|ip| cidr_whitelist_allows(&record.cidr_whitelist, ip));
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

/// This uses an allowlist for reads: `GET`, `HEAD`, and `OPTIONS` are
/// non-mutating. Safe read `POST` endpoints (resolver queries, lockfile
/// verification, artifact resolution/blob queries, OIDC handoffs, and security audits)
/// are non-mutating. Every other method or unlisted `POST` is considered a write.
pub(super) fn is_write_request(method: &Method, path: &str) -> bool {
    if matches!(*method, Method::GET | Method::HEAD | Method::OPTIONS) {
        return false;
    }
    if method.as_str() == "PROPFIND" && path.starts_with("/-/pnpr/v0/compiler-cache/") {
        return false;
    }
    if *method == Method::POST && is_read_only_post_path(path) {
        return false;
    }
    true
}

fn strip_registry_prefix(path: &str) -> &str {
    if let Some(rest) = path.strip_prefix("/~")
        && let Some(idx) = rest.find('/')
    {
        return &rest[idx..];
    }
    path
}

fn is_read_only_post_path(path: &str) -> bool {
    let path = path.trim_end_matches('/');
    let npm_path = path
        .strip_prefix(Ecosystem::Npm.base_path().as_str())
        .map(strip_registry_prefix);
    npm_path.is_some_and(|npm_path| {
        matches!(
            npm_path,
            "/-/npm/v1/security/advisories/bulk"
                | "/-/npm/v1/security/audits"
                | "/-/npm/v1/security/audits/quick",
        )
    }) || matches!(
        path,
        "/-/pnpr/v0/resolve"
            | "/-/pnpr/v0/verify-lockfile"
            | "/-/pnpr/v0/artifacts/resolve"
            | "/-/pnpr/v0/shared-artifacts/resolve"
            | "/-/pnpr/v0/artifacts/blob"
            | "/-/pnpr/v0/shared-artifacts/blob"
            | "/-/oidc/handoff",
    )
}

/// Whether `client_ip` falls inside any range of a token's CIDR whitelist.
pub(super) fn cidr_whitelist_allows(whitelist: &[String], client_ip: IpAddr) -> bool {
    whitelist.iter().any(|entry| cidr_contains(entry, client_ip))
}

/// Whether `client_ip` is inside one `addr/prefix` (or bare `addr`) whitelist
/// entry. A bare address matches only itself; a malformed entry (bad
/// address, or a non-numeric / out-of-range prefix) matches nothing, so
/// the restriction fails closed rather than open.
pub(super) fn cidr_contains(entry: &str, client_ip: IpAddr) -> bool {
    pnpr_config::IpNetwork::parse(entry.trim())
        .ok()
        .is_some_and(|net| net.contains(client_ip))
}
