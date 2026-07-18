use super::{
    ConnectInfo, PeerAddr, bearer_credentials, canonical_ip, cidr_contains, cidr_whitelist_allows,
    filter_registry_lane, is_write_method, router_with_auth, strip_registry_lane_dist_tags,
    token_timestamp_millis,
};
use crate::{
    auth::{AuthState, TokenBackend, TokenRecord, UserStore},
    config::Config,
    error::{RegistryError, Result},
    policy::{AccessList, PackageRule, PackageRules},
};
use async_trait::async_trait;
use axum::{
    body::{Body, to_bytes},
    http::{Method, Request, StatusCode, header},
};
use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    sync::Arc,
};
use tempfile::TempDir;
use tower::ServiceExt;

#[test]
fn registry_lanes_are_hidden_unless_requested() {
    let source = serde_json::json!({
        "dist-tags": {
            "latest": "1.0.0",
            "next": "0.0.0-next-20260718120000",
            "other": "0.0.0-other-20260718120000"
        },
        "versions": {
            "1.0.0": { "name": "example", "version": "1.0.0" },
            "0.0.0-next-20260718120000": {
                "name": "example",
                "version": "0.0.0-next-20260718120000",
                "_pnpmLane": "next"
            },
            "0.0.0-other-20260718120000": {
                "name": "example",
                "version": "0.0.0-other-20260718120000",
                "_pnpmLane": "other"
            }
        },
        "time": {
            "1.0.0": "2026-07-18T10:00:00Z",
            "0.0.0-next-20260718120000": "2026-07-18T12:00:00Z",
            "0.0.0-other-20260718120000": "2026-07-18T12:00:00Z"
        }
    });

    let mut default_packument = source.clone();
    filter_registry_lane(&mut default_packument, None);
    assert_eq!(
        default_packument["versions"].as_object().unwrap().keys().collect::<Vec<_>>(),
        vec!["1.0.0"],
    );
    assert_eq!(default_packument["dist-tags"], serde_json::json!({ "latest": "1.0.0" }));
    assert_eq!(default_packument["time"], serde_json::json!({ "1.0.0": "2026-07-18T10:00:00Z" }));

    let mut next_packument = source;
    filter_registry_lane(&mut next_packument, Some("next"));
    assert_eq!(
        next_packument["versions"].as_object().unwrap().keys().collect::<Vec<_>>(),
        vec!["1.0.0", "0.0.0-next-20260718120000"],
    );
    assert_eq!(
        next_packument["dist-tags"],
        serde_json::json!({
            "latest": "1.0.0",
            "next": "0.0.0-next-20260718120000"
        }),
    );
    assert!(next_packument["versions"]["0.0.0-next-20260718120000"].get("_pnpmLane").is_none());
}

#[test]
fn registry_lane_publish_does_not_update_a_default_dist_tag() {
    let mut incoming = serde_json::json!({
        "dist-tags": { "next": "0.0.0-next-20260718120000" },
        "versions": {
            "0.0.0-next-20260718120000": {
                "version": "0.0.0-next-20260718120000",
                "_pnpmLane": "next"
            }
        }
    });

    strip_registry_lane_dist_tags(&mut incoming);

    assert_eq!(incoming["dist-tags"], serde_json::json!({}));
}

#[test]
fn token_timestamp_millis_saturates_before_i64_conversion() {
    assert_eq!(token_timestamp_millis(42), 42_000);
    assert_eq!(token_timestamp_millis(u64::MAX), i64::MAX / 1000 * 1000);
}

// ---------------------------------------------------------------
// CIDR matching
// ---------------------------------------------------------------

fn ip(addr: &str) -> IpAddr {
    addr.parse().unwrap()
}

#[test]
fn cidr_contains_matches_ipv4_ranges() {
    assert!(cidr_contains("203.0.113.0/24", ip("203.0.113.7")));
    assert!(cidr_contains("203.0.113.0/24", ip("203.0.113.0"))); // first address
    assert!(cidr_contains("203.0.113.0/24", ip("203.0.113.255"))); // last address
    assert!(!cidr_contains("203.0.113.0/24", ip("203.0.114.1")));
}

#[test]
fn cidr_contains_handles_boundary_prefixes() {
    // /0 matches every IPv4 address.
    assert!(cidr_contains("0.0.0.0/0", ip("8.8.8.8")));
    // /32 matches only the exact host.
    assert!(cidr_contains("10.1.2.3/32", ip("10.1.2.3")));
    assert!(!cidr_contains("10.1.2.3/32", ip("10.1.2.4")));
}

#[test]
fn cidr_contains_treats_bare_address_as_exact_host() {
    assert!(cidr_contains("198.51.100.9", ip("198.51.100.9")));
    assert!(!cidr_contains("198.51.100.9", ip("198.51.100.10")));
}

#[test]
fn cidr_contains_matches_ipv6_ranges() {
    assert!(cidr_contains("2001:db8::/32", ip("2001:db8::1")));
    assert!(!cidr_contains("2001:db8::/32", ip("2001:db9::1")));
    assert!(cidr_contains("2001:db8::1/128", ip("2001:db8::1")));
    assert!(cidr_contains("::/0", ip("2001:db8::dead")));
}

#[test]
fn cidr_contains_rejects_family_mismatch_and_malformed_entries() {
    // An IPv4 peer never matches an IPv6 range, and vice versa.
    assert!(!cidr_contains("203.0.113.0/24", ip("2001:db8::1")));
    assert!(!cidr_contains("2001:db8::/32", ip("203.0.113.7")));
    // Malformed entries fail closed.
    assert!(!cidr_contains("not-an-address", ip("203.0.113.7")));
    assert!(!cidr_contains("203.0.113.0/33", ip("203.0.113.7")));
    assert!(!cidr_contains("203.0.113.0/-1", ip("203.0.113.7")));
    assert!(!cidr_contains("203.0.113.0/abc", ip("203.0.113.7")));
}

#[test]
fn canonical_ip_unwraps_ipv4_mapped_v6() {
    let mapped = IpAddr::V6("::ffff:203.0.113.7".parse::<Ipv6Addr>().unwrap());
    assert_eq!(canonical_ip(mapped), ip("203.0.113.7"));
    // A peer arriving as an IPv4-mapped IPv6 address still matches an
    // IPv4 whitelist range.
    assert!(cidr_whitelist_allows(&["203.0.113.0/24".to_string()], SocketAddr::new(mapped, 443),));
}

#[test]
fn cidr_whitelist_allows_requires_some_entry_to_match() {
    let whitelist = ["10.0.0.0/8".to_string(), "192.168.0.0/16".to_string()];
    assert!(cidr_whitelist_allows(&whitelist, SocketAddr::new(ip("10.9.9.9"), 1)));
    assert!(cidr_whitelist_allows(&whitelist, SocketAddr::new(ip("192.168.5.5"), 1)));
    assert!(!cidr_whitelist_allows(&whitelist, SocketAddr::new(ip("203.0.113.1"), 1)));
}

// ---------------------------------------------------------------
// Method classification and header parsing
// ---------------------------------------------------------------

#[test]
fn is_write_method_flags_only_mutating_methods() {
    assert!(is_write_method(&Method::PUT));
    assert!(is_write_method(&Method::DELETE));
    assert!(is_write_method(&Method::PATCH));
    assert!(!is_write_method(&Method::GET));
    assert!(!is_write_method(&Method::HEAD));
    assert!(!is_write_method(&Method::OPTIONS));
    assert!(!is_write_method(&Method::POST)); // resolver reads are POSTs
}

#[test]
fn bearer_credentials_extracts_only_bearer_tokens() {
    assert_eq!(bearer_credentials("Bearer abc123"), Some("abc123"));
    assert_eq!(bearer_credentials("bearer abc123"), Some("abc123")); // case-insensitive
    assert_eq!(bearer_credentials("  Bearer   abc123  "), Some("abc123"));
    assert_eq!(bearer_credentials("Basic dXNlcjpwYXNz"), None);
    assert_eq!(bearer_credentials("abc123"), None);
}

// ---------------------------------------------------------------
// End-to-end restriction enforcement
// ---------------------------------------------------------------

const PEER: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(10, 1, 2, 3)), 40000);

fn record(readonly: bool, cidr_whitelist: &[&str]) -> TokenRecord {
    TokenRecord {
        username: "alice".to_string(),
        created_at: 1,
        last_used_at: 1,
        readonly,
        cidr_whitelist: cidr_whitelist.iter().map(|entry| (*entry).to_string()).collect(),
    }
}

/// Token backend that resolves exactly one raw token to a fixed record.
/// No client endpoint mints read-only or CIDR-restricted tokens, so the
/// enforcement tests inject one this way.
struct OneToken {
    raw: String,
    record: TokenRecord,
}

#[async_trait]
impl TokenBackend for OneToken {
    async fn issue(&self, _username: &str) -> Result<String> {
        // The restriction tests never issue tokens; surface a clear error
        // rather than a panic if that assumption ever breaks.
        Err(RegistryError::Internal { reason: "OneToken cannot issue tokens".to_string() })
    }

    async fn lookup(&self, raw: &str) -> Result<Option<String>> {
        Ok((raw == self.raw).then(|| self.record.username.clone()))
    }

    async fn lookup_record(&self, raw: &str) -> Result<Option<TokenRecord>> {
        Ok((raw == self.raw).then(|| self.record.clone()))
    }

    async fn find_by_key(&self, _key: &str) -> Result<Option<TokenRecord>> {
        Ok(None)
    }

    async fn list_for_user(&self, _username: &str) -> Result<Vec<(String, TokenRecord)>> {
        Ok(Vec::new())
    }

    async fn revoke_by_key(&self, _key: &str) -> Result<Option<TokenRecord>> {
        Ok(None)
    }
}

fn app_with_token(tmp: &TempDir, raw: &str, record: TokenRecord) -> axum::Router {
    let listen = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0);
    app_with_config_and_token(Config::static_serve(listen, tmp.path().to_path_buf()), raw, record)
}

fn app_with_config_and_token(config: Config, raw: &str, record: TokenRecord) -> axum::Router {
    let tokens: Arc<dyn TokenBackend> = Arc::new(OneToken { raw: raw.to_string(), record });
    let auth = AuthState { users: Arc::new(UserStore::in_memory()), tokens };
    router_with_auth(config, auth)
}

fn signed(method: Method, path: &str, raw: &str) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(path)
        .header(header::AUTHORIZATION, format!("Bearer {raw}"))
        .body(Body::empty())
        .unwrap()
}

fn with_peer(mut request: Request<Body>, addr: SocketAddr) -> Request<Body> {
    request.extensions_mut().insert(ConnectInfo(PeerAddr(addr)));
    request
}

async fn status(app: axum::Router, request: Request<Body>) -> StatusCode {
    app.oneshot(request).await.unwrap().status()
}

#[tokio::test]
async fn authenticated_identity_reaches_handlers() {
    let tmp = TempDir::new().unwrap();
    let app = app_with_token(&tmp, "tok", record(false, &[]));
    // The middleware resolves the bearer once; whoami reads that identity
    // back out of request extensions rather than re-parsing the header.
    let response = app.clone().oneshot(signed(Method::GET, "/-/whoami", "tok")).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["username"], "alice");

    // An unknown token resolves to anonymous, so whoami is a 401.
    let anon = app.oneshot(signed(Method::GET, "/-/whoami", "unknown")).await.unwrap();
    assert_eq!(anon.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn team_tokens_reach_package_authorization() {
    let tmp = TempDir::new().unwrap();
    let listen = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0);
    let mut config = Config::static_serve(listen, tmp.path().to_path_buf());
    use crate::policy::{AccessToken, Identity};
    use crate::registry::PackagePattern;
    config.hosted.get_mut("local").unwrap().rules = PackageRules::new(
        vec![PackageRule {
            pattern: PackagePattern::parse("@team/*").unwrap(),
            access: Some(AccessList::new(vec![AccessToken::Team {
                name: "platform".to_string(),
                members: ["alice".to_string()].into(),
            }])),
            publish: None,
            unpublish: None,
        }],
        None,
    );
    // Team membership reaches the per-package rule evaluation.
    let alice = Identity::user("alice");
    let carol = Identity::user("carol");
    let rules = &config.hosted["local"].rules;
    assert!(rules.for_package("@team/x").access.allows(&alice));
    assert!(!rules.for_package("@team/x").access.allows(&carol));

    // Over HTTP: the team member reaches storage (404, the package is
    // absent); a caller denied by the *explicit* `@team/*` entry is
    // rejected loudly — 401 for anonymous, so clients can prompt for
    // credentials — rather than masked (masking is the registry-level
    // default's behavior, not an explicit entry's).
    let app = app_with_config_and_token(config, "tok", record(false, &[]));
    let allowed = app.clone().oneshot(signed(Method::GET, "/@team/missing", "tok")).await.unwrap();
    assert_eq!(allowed.status(), StatusCode::NOT_FOUND);
    let anonymous = app.oneshot(signed(Method::GET, "/@team/missing", "unknown")).await.unwrap();
    assert_eq!(anonymous.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn readonly_token_is_refused_for_writes() {
    let tmp = TempDir::new().unwrap();
    let app = app_with_token(&tmp, "ro", record(true, &[]));
    // Publish (PUT) and unpublish (DELETE) are rejected before the
    // handler ever reads the body.
    assert_eq!(status(app.clone(), signed(Method::PUT, "/foo", "ro")).await, StatusCode::FORBIDDEN);
    assert_eq!(
        status(app, signed(Method::DELETE, "/foo/-rev/1", "ro")).await,
        StatusCode::FORBIDDEN,
    );
}

#[tokio::test]
async fn readonly_token_still_reads() {
    let tmp = TempDir::new().unwrap();
    let app = app_with_token(&tmp, "ro", record(true, &[]));
    // A GET is not a write, so the read-only gate lets it through; the
    // package simply isn't published, so it 404s rather than 403s.
    assert_eq!(status(app, signed(Method::GET, "/foo", "ro")).await, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn unrestricted_token_passes_the_gate_for_writes() {
    let tmp = TempDir::new().unwrap();
    let app = app_with_token(&tmp, "rw", record(false, &[]));
    // The gate doesn't block an unrestricted token's write: the request
    // reaches the publish handler (which then rejects the empty body on
    // its own terms). The point is that it is not a 403 from the gate.
    let status = status(app, signed(Method::PUT, "/foo", "rw")).await;
    assert_ne!(status, StatusCode::FORBIDDEN);
    assert_ne!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn cidr_token_is_refused_when_peer_is_unknown() {
    let tmp = TempDir::new().unwrap();
    let app = app_with_token(&tmp, "pinned", record(false, &["10.0.0.0/8"]));
    // No ConnectInfo on the request: the restriction can't be checked, so
    // it fails closed even for a read.
    assert_eq!(status(app, signed(Method::GET, "/foo", "pinned")).await, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn cidr_token_is_refused_from_outside_the_range() {
    let tmp = TempDir::new().unwrap();
    let app = app_with_token(&tmp, "pinned", record(false, &["10.0.0.0/8"]));
    let outside = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 5)), 40000);
    assert_eq!(
        status(app, with_peer(signed(Method::GET, "/foo", "pinned"), outside)).await,
        StatusCode::FORBIDDEN,
    );
}

#[tokio::test]
async fn cidr_token_is_allowed_from_inside_the_range() {
    let tmp = TempDir::new().unwrap();
    let app = app_with_token(&tmp, "pinned", record(false, &["10.0.0.0/8"]));
    assert_eq!(
        status(app, with_peer(signed(Method::GET, "/foo", "pinned"), PEER)).await,
        StatusCode::NOT_FOUND,
    );
}

#[tokio::test]
async fn forwarded_header_cannot_satisfy_a_cidr_restriction() {
    let tmp = TempDir::new().unwrap();
    let app = app_with_token(&tmp, "pinned", record(false, &["10.0.0.0/8"]));
    // A spoofed X-Forwarded-For for an in-range address must not help: the
    // gate reads the socket peer (here absent), never the header.
    let request = signed(Method::GET, "/foo", "pinned");
    let request = {
        let mut request = request;
        request.headers_mut().insert("x-forwarded-for", "10.1.2.3".parse().unwrap());
        request
    };
    assert_eq!(status(app, request).await, StatusCode::FORBIDDEN);
}

#[test]
fn access_log_uri_redacts_the_logout_token_segment() {
    let redact = |raw: &str| super::loggable_uri(&raw.parse().unwrap());
    assert_eq!(redact("/-/user/token/npm_secret-token-value"), "/-/user/token/<redacted>");
    assert_eq!(
        redact("/~corp/-/user/token/npm_secret-token-value"),
        "/~corp/-/user/token/<redacted>",
    );
    // Everything else is logged verbatim, query string included.
    assert_eq!(redact("/foo/-/foo-1.0.0.tgz"), "/foo/-/foo-1.0.0.tgz");
    assert_eq!(redact("/-/v1/search?text=foo"), "/-/v1/search?text=foo");
}

// --------------------------------------------------------------------
// npm team API — listings from the config-declared `teams:` map,
// config-managed rejection for mutations, not-found masking for callers
// the registry-level `access` denies.
// --------------------------------------------------------------------

fn config_with_teams(tmp: &TempDir) -> Config {
    let listen = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0);
    let mut config = Config::static_serve(listen, tmp.path().to_path_buf());
    let teams = [("developers", vec!["bob", "alice"]), ("admins", vec!["alice"])];
    config.hosted.get_mut("local").unwrap().teams = teams
        .into_iter()
        .map(|(team, members)| {
            (team.to_string(), members.into_iter().map(str::to_string).collect())
        })
        .collect();
    config
}

async fn body_json(response: axum::response::Response) -> serde_json::Value {
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    serde_json::from_slice(&body).unwrap()
}

#[tokio::test]
async fn team_listing_serves_config_declared_teams() {
    let tmp = TempDir::new().unwrap();
    let app = app_with_config_and_token(config_with_teams(&tmp), "tok", record(false, &[]));
    // Declaration order is preserved; the shape is what `pnpm team ls`
    // consumes.
    let expected = serde_json::json!([{ "name": "developers" }, { "name": "admins" }]);
    let response =
        app.clone().oneshot(signed(Method::GET, "/-/org/myorg/team", "tok")).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body_json(response).await, expected);
    // The same listing through the registry-addressed endpoint.
    let prefixed =
        app.oneshot(signed(Method::GET, "/~local/-/org/myorg/team", "tok")).await.unwrap();
    assert_eq!(prefixed.status(), StatusCode::OK);
    assert_eq!(body_json(prefixed).await, expected);
}

#[tokio::test]
async fn team_members_are_listed_sorted() {
    let tmp = TempDir::new().unwrap();
    let app = app_with_config_and_token(config_with_teams(&tmp), "tok", record(false, &[]));
    // Members come from a sorted set, not declaration order.
    let expected = serde_json::json!([{ "name": "alice" }, { "name": "bob" }]);
    let response = app
        .clone()
        .oneshot(signed(Method::GET, "/-/team/myorg/developers/user", "tok"))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body_json(response).await, expected);
    let prefixed = app
        .clone()
        .oneshot(signed(Method::GET, "/~local/-/team/myorg/developers/user", "tok"))
        .await
        .unwrap();
    assert_eq!(prefixed.status(), StatusCode::OK);
    assert_eq!(body_json(prefixed).await, expected);
    // A team the config does not declare is a definitive not-found.
    let missing = app.oneshot(signed(Method::GET, "/-/team/myorg/nope/user", "tok")).await.unwrap();
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn team_mutations_are_rejected_as_config_managed() {
    let tmp = TempDir::new().unwrap();
    let app = app_with_config_and_token(config_with_teams(&tmp), "tok", record(false, &[]));
    let mutations = [
        (Method::PUT, "/-/org/myorg/team"),
        (Method::DELETE, "/-/team/myorg/developers"),
        (Method::PUT, "/-/team/myorg/developers/user"),
        (Method::DELETE, "/-/team/myorg/developers/user"),
        (Method::PUT, "/~local/-/org/myorg/team"),
        (Method::DELETE, "/~local/-/team/myorg/developers"),
        (Method::PUT, "/~local/-/team/myorg/developers/user"),
        (Method::DELETE, "/~local/-/team/myorg/developers/user"),
    ];
    for (method, path) in mutations {
        let response = app.clone().oneshot(signed(method.clone(), path, "tok")).await.unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN, "{method} {path}");
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body = String::from_utf8(body.to_vec()).unwrap();
        assert!(body.contains("declared in the pnpr configuration"), "{method} {path}: {body}");
    }
}

#[tokio::test]
async fn team_listing_masks_callers_the_registry_denies() {
    let tmp = TempDir::new().unwrap();
    let mut config = config_with_teams(&tmp);
    // Registry-level default access admits only authenticated callers.
    config.hosted.get_mut("local").unwrap().rules =
        PackageRules::new(Vec::new(), Some(AccessList::from_tokens(["$authenticated"])));
    let app = app_with_config_and_token(config, "tok", record(false, &[]));
    // An anonymous caller gets the not-found mask on reads and mutations
    // alike — team names must not become an existence probe.
    for (method, path) in [
        (Method::GET, "/-/org/myorg/team"),
        (Method::GET, "/-/team/myorg/developers/user"),
        (Method::PUT, "/-/org/myorg/team"),
    ] {
        let response = app.clone().oneshot(signed(method.clone(), path, "unknown")).await.unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{method} {path}");
    }
    // The authenticated caller passes the gate.
    let allowed = app.oneshot(signed(Method::GET, "/-/org/myorg/team", "tok")).await.unwrap();
    assert_eq!(allowed.status(), StatusCode::OK);
}
