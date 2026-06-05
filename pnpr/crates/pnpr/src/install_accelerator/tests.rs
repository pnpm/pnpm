//! Tests for the pnpr-as-authority access gate the install accelerator
//! applies before serving a package's files: a digest in the store is not
//! a bearer capability, so [`deny_local_policy`] checks every locally-
//! authoritative package against pnpr's own `packages:` policy. (The
//! upstream-as-authority regime — forwarded-credential content gated per
//! user — is exercised end to end in the pnpr-client integration tests.)

use std::collections::HashSet;

use axum::http::StatusCode;
use pacquet_network::AuthHeaders;
use pacquet_store_dir::StoreDir;
use tempfile::TempDir;

use super::{
    InstallAccelerator, authorize_served_packages, authorize_upstream_package, deny_local_policy,
    diff::PackageIndexEntry, protocol::InstallRequest, validate_files,
};
use crate::policy::{AccessList, Identity, PackagePolicies, PackagePolicy};

/// The `name@1.0.0` package id a served entry would carry.
fn served(name: &str) -> String {
    format!("{name}@1.0.0")
}

/// Run the local-policy gate over a single served package id.
fn deny(
    policies: &PackagePolicies,
    identity: &Identity,
    pkg_id: &str,
) -> Option<axum::response::Response> {
    deny_local_policy(policies, identity, std::iter::once(pkg_id))
}

fn anonymous() -> Identity {
    Identity::Anonymous
}

fn user() -> Identity {
    Identity::User { username: "alice".to_string() }
}

/// `registry_mock_defaults` gates `@private/*` to `$authenticated`.
fn policies() -> PackagePolicies {
    PackagePolicies::registry_mock_defaults()
}

/// `@team/*` is restricted to the single user `alice`, so an authenticated
/// caller who isn't `alice` is forbidden rather than merely unauthenticated.
fn team_owned_by_alice() -> PackagePolicies {
    let team =
        PackagePolicy::new("@team/*", AccessList::parse("alice"), AccessList::parse("alice"))
            .expect("pattern compiles");
    let rest =
        PackagePolicy::new("**", AccessList::parse("$all"), AccessList::parse("$authenticated"))
            .expect("pattern compiles");
    PackagePolicies::new(vec![team, rest])
}

#[test]
fn anonymous_caller_is_denied_a_private_package() {
    let denied = deny(&policies(), &anonymous(), &served("@private/foo"));
    assert_eq!(denied.map(|response| response.status()), Some(StatusCode::UNAUTHORIZED));
}

#[test]
fn authenticated_caller_is_allowed_a_private_package() {
    let denied = deny(&policies(), &user(), &served("@private/foo"));
    assert!(denied.is_none());
}

#[test]
fn anonymous_caller_is_allowed_a_public_package() {
    let denied = deny(&policies(), &anonymous(), &served("is-positive"));
    assert!(denied.is_none());
}

#[test]
fn authenticated_caller_outside_the_allowed_set_is_forbidden() {
    let bob = Identity::User { username: "bob".to_string() };
    let denied = deny(&team_owned_by_alice(), &bob, &served("@team/foo"));
    assert_eq!(denied.map(|response| response.status()), Some(StatusCode::FORBIDDEN));
}

#[test]
fn authenticated_caller_in_the_allowed_set_is_allowed() {
    let denied = deny(&team_owned_by_alice(), &user(), &served("@team/foo"));
    assert!(denied.is_none());
}

// --------------------------------------------------------------------
// Upstream-as-authority regime: forwarded-credential content gated per
// user against the owning external registry, plus the grant table.
// --------------------------------------------------------------------

/// Build a real [`InstallAccelerator`] (store/cache dirs + grant table)
/// under `storage` for the dispatch tests.
fn accelerator(storage: &std::path::Path) -> InstallAccelerator {
    let addr = "127.0.0.1:4873".parse().expect("addr parses");
    let config = crate::config::Config::proxy(addr, storage.to_path_buf());
    InstallAccelerator::build(&config)
}

/// An [`AuthHeaders`] carrying a single default-registry credential for
/// `registry`, mirroring how a client forwards one upstream token.
fn auth_for(registry: &str, header: &str) -> AuthHeaders {
    AuthHeaders::from_creds_map([(String::new(), header.to_string())], Some(registry))
}

fn entry(pkg_id: &str) -> PackageIndexEntry {
    PackageIndexEntry {
        integrity: "sha512-x".to_string(),
        pkg_id: pkg_id.to_string(),
        raw: Vec::new(),
    }
}

fn is_granted(acc: &InstallAccelerator, user: &str, pkg: &str) -> bool {
    acc.grant_table.as_ref().expect("grant table opened").is_granted(user, pkg, None)
}

fn is_public(acc: &InstallAccelerator, name: &str) -> bool {
    acc.public_packages.as_ref().expect("public set opened").is_public(name, None)
}

fn fresh(pkg_ids: &[&str]) -> HashSet<String> {
    pkg_ids.iter().map(|id| id.to_string()).collect()
}

#[tokio::test]
async fn a_fresh_upstream_fetch_is_allowed_and_records_a_grant() {
    let tmp = TempDir::new().unwrap();
    let acc = accelerator(tmp.path());
    let auth = auth_for("https://reg.test/", "Bearer t");
    let identity = Identity::User { username: "alice".to_string() };

    let denied = authorize_upstream_package(
        &acc,
        &identity,
        &auth,
        &fresh(&["foo@1.0.0"]),
        "https://reg.test/",
        "foo",
        "foo@1.0.0",
    )
    .await;

    assert!(denied.is_none());
    assert!(is_granted(&acc, "alice", "foo@1.0.0"));
}

#[tokio::test]
async fn a_granted_cache_hit_is_served_without_touching_the_upstream() {
    let tmp = TempDir::new().unwrap();
    let acc = accelerator(tmp.path());
    acc.grant_table.as_ref().unwrap().record("alice", "foo@1.0.0");
    let auth = auth_for("https://reg.test/", "Bearer t");
    let identity = Identity::User { username: "alice".to_string() };

    // An unreachable registry: a network probe would resolve to a 502
    // denial, so a pass here proves the grant short-circuited it.
    let denied = authorize_upstream_package(
        &acc,
        &identity,
        &auth,
        &fresh(&[]),
        "http://127.0.0.1:1/",
        "foo",
        "foo@1.0.0",
    )
    .await;

    assert!(denied.is_none());
}

#[tokio::test]
async fn an_ungranted_private_cache_hit_reverifies_then_records() {
    let mut server = mockito::Server::new_async().await;
    // Private: the registry withholds the packument anonymously, then
    // serves it once the caller's credential is attached. The two mocks
    // are mutually exclusive on the `authorization` header.
    let anon = server
        .mock("GET", "/foo")
        .match_header("authorization", mockito::Matcher::Missing)
        .with_status(401)
        .create_async()
        .await;
    let authed = server
        .mock("GET", "/foo")
        .match_header("authorization", "Bearer t")
        .with_status(200)
        .with_body("{}")
        .create_async()
        .await;
    let registry = format!("{}/", server.url());

    let tmp = TempDir::new().unwrap();
    let acc = accelerator(tmp.path());
    let auth = auth_for(&registry, "Bearer t");
    let identity = Identity::User { username: "alice".to_string() };

    let denied = authorize_upstream_package(
        &acc,
        &identity,
        &auth,
        &fresh(&[]),
        &registry,
        "foo",
        "foo@1.0.0",
    )
    .await;

    assert!(denied.is_none());
    anon.assert_async().await;
    authed.assert_async().await;
    assert!(is_granted(&acc, "alice", "foo@1.0.0"));
    // A private package must never be cached as public.
    assert!(!is_public(&acc, "foo"));
}

#[tokio::test]
async fn a_public_cache_hit_is_classified_once_then_served_for_free() {
    let mut server = mockito::Server::new_async().await;
    // Public: the registry serves the packument anonymously. Exactly one
    // probe is expected across both authorize calls — the second is served
    // from the global classification with no upstream contact.
    let mock =
        server.mock("GET", "/foo").with_status(200).with_body("{}").expect(1).create_async().await;
    let registry = format!("{}/", server.url());

    let tmp = TempDir::new().unwrap();
    let acc = accelerator(tmp.path());
    let auth = auth_for(&registry, "Bearer t");
    let alice = Identity::User { username: "alice".to_string() };

    let first =
        authorize_upstream_package(&acc, &alice, &auth, &fresh(&[]), &registry, "foo", "foo@1.0.0")
            .await;
    assert!(first.is_none());
    assert!(is_public(&acc, "foo"));
    // Public content records no per-user grant.
    assert!(!is_granted(&acc, "alice", "foo@1.0.0"));

    // A different caller wanting a different cached version is served
    // straight from the classification — no second probe.
    let bob = Identity::User { username: "bob".to_string() };
    let second =
        authorize_upstream_package(&acc, &bob, &auth, &fresh(&[]), &registry, "foo", "foo@2.0.0")
            .await;
    assert!(second.is_none());

    mock.assert_async().await;
}

#[tokio::test]
async fn a_denied_reverify_clears_the_users_grants_and_denies() {
    let mut server = mockito::Server::new_async().await;
    let _mock = server.mock("GET", "/foo").with_status(403).create_async().await;
    let registry = format!("{}/", server.url());

    let tmp = TempDir::new().unwrap();
    let acc = accelerator(tmp.path());
    // A standing grant for another version the caller already held: a
    // discovered `403` for the package must purge it (clear-on-discovery).
    acc.grant_table.as_ref().unwrap().record("alice", "foo@2.0.0");
    let auth = auth_for(&registry, "Bearer t");
    let identity = Identity::User { username: "alice".to_string() };

    let denied = authorize_upstream_package(
        &acc,
        &identity,
        &auth,
        &fresh(&[]),
        &registry,
        "foo",
        "foo@1.0.0",
    )
    .await;

    assert_eq!(denied.map(|response| response.status()), Some(StatusCode::FORBIDDEN));
    assert!(!is_granted(&acc, "alice", "foo@2.0.0"));
}

#[tokio::test]
async fn an_unreachable_upstream_during_reverify_is_a_bad_gateway() {
    let tmp = TempDir::new().unwrap();
    let acc = accelerator(tmp.path());
    let auth = auth_for("http://127.0.0.1:1/", "Bearer t");
    let identity = Identity::User { username: "alice".to_string() };

    // Port 1 refuses the connection, so neither the anonymous classify
    // probe nor the authed re-verify can decide access.
    let denied = authorize_upstream_package(
        &acc,
        &identity,
        &auth,
        &fresh(&[]),
        "http://127.0.0.1:1/",
        "foo",
        "foo@1.0.0",
    )
    .await;

    assert_eq!(denied.map(|response| response.status()), Some(StatusCode::BAD_GATEWAY));
}

#[tokio::test]
async fn a_forwarded_credential_routes_around_the_local_policy() {
    // `@private/foo` is gated to `$authenticated` by the local policy, so
    // an anonymous caller would be denied under pnpr-as-authority. With a
    // forwarded credential it is upstream-as-authority instead, and a
    // fresh fetch proves access — so it is served.
    let tmp = TempDir::new().unwrap();
    let acc = accelerator(tmp.path());
    let registry = "https://reg.test/";
    let auth = auth_for(registry, "Bearer t");
    let request = InstallRequest { registry: Some(registry.to_string()), ..Default::default() };

    let denied = authorize_served_packages(
        &acc,
        &policies(),
        &Identity::Anonymous,
        &request,
        &auth,
        &fresh(&["@private/foo@1.0.0"]),
        &[entry("@private/foo@1.0.0")],
    )
    .await;

    assert!(denied.is_none());
}

#[tokio::test]
async fn without_a_forwarded_credential_the_local_policy_still_applies() {
    let tmp = TempDir::new().unwrap();
    let acc = accelerator(tmp.path());
    let request =
        InstallRequest { registry: Some("https://reg.test/".to_string()), ..Default::default() };

    // No forwarded credential ⇒ pnpr-as-authority ⇒ `@private/foo` is
    // denied to an anonymous caller, exactly as the packument/tarball
    // endpoints would deny it.
    let denied = authorize_served_packages(
        &acc,
        &policies(),
        &Identity::Anonymous,
        &request,
        &AuthHeaders::default(),
        &fresh(&[]),
        &[entry("@private/foo@1.0.0")],
    )
    .await;

    assert_eq!(denied.map(|response| response.status()), Some(StatusCode::UNAUTHORIZED));
}

/// A valid 128-hex-char digest whose first two chars shard the CAS path.
fn digest(byte: &str) -> String {
    byte.repeat(64)
}

/// Write `content` at the non-executable CAS path for `digest` so a later
/// `validate_files` finds it.
fn write_cas(store_dir: &StoreDir, digest: &str, content: &[u8]) {
    let path = store_dir.cas_file_path_by_mode(digest, 0o644).expect("resolvable digest");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, content).unwrap();
}

/// A present, readable file resolves to a `ValidatedFile` carrying its CAS
/// path and decoded digest bytes.
#[test]
fn validate_files_accepts_a_present_file() {
    let tmp = TempDir::new().unwrap();
    let store_dir = StoreDir::new(tmp.path().to_path_buf());
    let digest = digest("ab");
    write_cas(&store_dir, &digest, b"console.log(1)");

    let validated = validate_files(&store_dir, &[(digest, false)]).expect("present file");
    assert_eq!(validated.len(), 1);
    assert_eq!(validated[0].digest_bytes, [0xab; 64]);
    assert!(!validated[0].executable);
    assert_eq!(std::fs::read(&validated[0].path).unwrap(), b"console.log(1)");
}

/// A digest the diff produced but whose file is absent from the store
/// fails *before* the streamed `200`, as a `500` — not a truncated body.
/// This is the regression the up-front manifest validation guards against.
#[test]
fn validate_files_rejects_a_missing_store_file() {
    let tmp = TempDir::new().unwrap();
    let store_dir = StoreDir::new(tmp.path().to_path_buf());

    let Err((status, _)) = validate_files(&store_dir, &[(digest("cd"), false)]) else {
        panic!("a missing store file must fail validation");
    };
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
}

/// A digest that resolves to a CAS path but isn't 64 bytes of hex is a
/// `400`, decoded up front rather than mid-stream.
#[test]
fn validate_files_rejects_a_malformed_digest() {
    let tmp = TempDir::new().unwrap();
    let store_dir = StoreDir::new(tmp.path().to_path_buf());

    let Err((status, _)) = validate_files(&store_dir, &[("abc".to_string(), false)]) else {
        panic!("a malformed digest must fail validation");
    };
    assert_eq!(status, StatusCode::BAD_REQUEST);
}
