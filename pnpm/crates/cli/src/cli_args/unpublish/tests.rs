use mockito::Matcher;
use pnpm_config::Config;
use pnpm_network_web_auth_testing::{InputResponse, ok_token, web_auth_fake};
use serde_json::{Map, Value, json};

use super::{
    Packument, UnpublishArgs, highest_version, registry_origin, rev_str, tarball_pathname,
    versions_matching_range,
};

fn versions(keys: &[&str]) -> Map<String, Value> {
    keys.iter().map(|key| ((*key).to_string(), json!({}))).collect()
}

#[test]
fn rev_str_falls_back_to_the_undefined_literal() {
    assert_eq!(rev_str(Some("3-abc")), "3-abc");
    // A packument without _rev renders like the TypeScript template string.
    assert_eq!(rev_str(None), "undefined");
}

#[test]
fn versions_matching_range_mirrors_semver_satisfies() {
    let versions = versions(&["1.0.0", "1.5.0", "2.0.0"]);
    assert_eq!(versions_matching_range(&versions, "1.0.0"), ["1.0.0"]);
    assert_eq!(versions_matching_range(&versions, "^1.0.0"), ["1.0.0", "1.5.0"]);
    assert_eq!(versions_matching_range(&versions, ">=1.0.0"), ["1.0.0", "1.5.0", "2.0.0"]);
    assert!(versions_matching_range(&versions, "9.9.9").is_empty(), "no match yields empty");
    assert!(versions_matching_range(&versions, "not a range").is_empty(), "junk matches nothing");
}

#[test]
fn highest_version_picks_the_new_latest() {
    assert_eq!(
        highest_version(&versions(&["1.0.0", "10.0.0", "9.0.0"])).as_deref(),
        Some("10.0.0"),
    );
    assert_eq!(
        highest_version(&versions(&["1.0.0", "1.0.1-beta.1"])).as_deref(),
        Some("1.0.1-beta.1"),
    );
    assert_eq!(highest_version(&versions(&[])), None);
}

#[test]
fn registry_origin_drops_the_registry_path() {
    let origin = registry_origin("https://registry.example.com:8443/npm/").expect("an origin");
    assert_eq!(origin, "https://registry.example.com:8443");
}

#[test]
fn tarball_pathname_strips_the_registry_path_prefix() {
    // A registry at the host root keeps the tarball path as is.
    let pathname = tarball_pathname(
        "https://registry.example.com/pkg/-/pkg-1.0.0.tgz",
        "https://registry.example.com/",
    )
    .expect("a pathname");
    assert_eq!(pathname, "pkg/-/pkg-1.0.0.tgz");

    // A registry mounted under a path is stripped from the tarball path.
    let pathname = tarball_pathname(
        "https://registry.example.com/npm/pkg/-/pkg-1.0.0.tgz",
        "https://registry.example.com/npm/",
    )
    .expect("a pathname");
    assert_eq!(pathname, "pkg/-/pkg-1.0.0.tgz");
}

/// The packument round-trips unknown fields and drops the `CouchDB` metadata
/// keys the way the PUT body requires.
#[test]
fn packument_round_trips_unknown_fields() {
    let raw = json!({
        "name": "pkg",
        "_rev": "3-abc",
        "dist-tags": { "latest": "1.0.0" },
        "versions": { "1.0.0": { "dist": { "tarball": "https://x/pkg-1.0.0.tgz" } } },
        "readme": "hello",
        "_revisions": { "start": 3 },
        "_attachments": {},
    });
    let mut packument: Packument = serde_json::from_value(raw).expect("a packument deserializes");
    assert_eq!(packument.rev.as_deref(), Some("3-abc"));

    packument.other.remove("_revisions");
    packument.other.remove("_attachments");
    let serialized = serde_json::to_value(&packument).expect("a packument serializes");
    assert_eq!(serialized.get("readme"), Some(&json!("hello")), "unknown fields survive");
    assert!(serialized.get("_revisions").is_none(), "couchdb metadata is dropped");
    assert!(serialized.get("_attachments").is_none(), "couchdb metadata is dropped");
}

/// The version keys keep the packument's own order — `1.10.0` after `1.9.0`
/// when the registry sent them that way — like the TypeScript CLI's
/// `Object.keys`, not lexicographic or semver order.
#[test]
fn version_keys_keep_the_packument_order() {
    let packument: Packument = serde_json::from_value(json!({
        "name": "pkg",
        "versions": { "1.9.0": {}, "1.10.0": {}, "1.2.0": {} },
    }))
    .expect("a packument deserializes");
    let keys: Vec<&String> = packument.versions.keys().collect();
    assert_eq!(keys, ["1.9.0", "1.10.0", "1.2.0"], "insertion order survives");
}

/// A two-version packument whose tarballs live on `server_url`.
fn two_version_packument(server_url: &str) -> String {
    json!({
        "name": "test-pkg",
        "_rev": "3-abc",
        "dist-tags": { "latest": "0.0.2" },
        "versions": {
            "0.0.1": { "dist": { "tarball": format!("{server_url}/test-pkg/-/test-pkg-0.0.1.tgz") } },
            "0.0.2": { "dist": { "tarball": format!("{server_url}/test-pkg/-/test-pkg-0.0.2.tgz") } },
        },
    })
    .to_string()
}

fn unpublish_args(registry: &str, otp: Option<&str>, params: &[&str]) -> UnpublishArgs {
    UnpublishArgs {
        registry: Some(registry.to_owned()),
        otp: otp.map(str::to_owned),
        force: true,
        params: params.iter().map(|param| (*param).to_owned()).collect(),
    }
}

/// A 401 carrying `authUrl` / `doneUrl` starts the web-auth flow, and the
/// token it yields is sent as `npm-otp` on the retried `DELETE` — still
/// under `npm-auth-type: web`.
#[tokio::test]
async fn a_web_auth_challenge_is_answered_and_the_delete_retried_with_the_token() {
    web_auth_fake!(FakeHost, RecordingReporter, set_fetch);
    reset();
    set_fetch(Box::new(|| Ok(ok_token("web-token"))));

    let mut server = mockito::Server::new_async().await;
    let registry = format!("{}/", server.url());
    let get_mock = server
        .mock("GET", "/test-pkg")
        .with_status(200)
        .with_body(two_version_packument(&server.url()))
        .create_async()
        .await;
    let challenge_mock = server
        .mock("DELETE", "/test-pkg/-rev/3-abc")
        .match_header("npm-auth-type", "web")
        .match_header("npm-otp", Matcher::Missing)
        .with_status(401)
        .with_body(
            json!({
                "error": "one-time pass required",
                "authUrl": "https://auth.example/login",
                "doneUrl": "https://auth.example/done",
            })
            .to_string(),
        )
        .create_async()
        .await;
    let retry_mock = server
        .mock("DELETE", "/test-pkg/-rev/3-abc")
        .match_header("npm-auth-type", "web")
        .match_header("npm-otp", "web-token")
        .with_status(200)
        .with_body("{}")
        .create_async()
        .await;

    let output = unpublish_args(&registry, None, &["test-pkg"])
        .execute::<FakeHost, RecordingReporter>(&Config::default())
        .await
        .expect("the challenge is answered and the unpublish succeeds");

    assert_eq!(output, "Successfully unpublished all 2 version(s) of test-pkg");
    get_mock.assert_async().await;
    challenge_mock.assert_async().await;
    retry_mock.assert_async().await;
}

/// The one-time password a classic challenge yields is kept for the rest of
/// the run: the tarball `DELETE` that follows the packument `PUT` carries it
/// without a second prompt.
#[tokio::test]
async fn a_partial_unpublish_shares_one_otp_across_the_put_and_the_tarball_delete() {
    web_auth_fake!(FakeHost, RecordingReporter, set_input);
    reset();
    set_input(InputResponse::Value(Some("123456".to_owned())));

    let mut server = mockito::Server::new_async().await;
    let registry = format!("{}/", server.url());
    let get_mock = server
        .mock("GET", "/test-pkg")
        .with_status(200)
        .with_body(two_version_packument(&server.url()))
        .expect(2)
        .create_async()
        .await;
    let challenge_mock = server
        .mock("PUT", "/test-pkg/-rev/3-abc")
        .match_header("npm-otp", Matcher::Missing)
        .with_status(401)
        .with_body(r#"{"error":"You must provide a one-time pass. Upgrade your client to npm@latest in order to use 2FA."}"#)
        .create_async()
        .await;
    let put_mock = server
        .mock("PUT", "/test-pkg/-rev/3-abc")
        .match_header("npm-otp", "123456")
        .with_status(200)
        .with_body("{}")
        .create_async()
        .await;
    let tarball_mock = server
        .mock("DELETE", "/test-pkg/-/test-pkg-0.0.1.tgz/-rev/3-abc")
        .match_header("npm-otp", "123456")
        .with_status(200)
        .with_body("{}")
        .create_async()
        .await;

    let output = unpublish_args(&registry, None, &["test-pkg@0.0.1"])
        .execute::<FakeHost, RecordingReporter>(&Config::default())
        .await
        .expect("the challenge is answered once and the unpublish succeeds");

    assert_eq!(output, "Successfully unpublished 1 version(s) of test-pkg");
    get_mock.assert_async().await;
    challenge_mock.assert_async().await;
    put_mock.assert_async().await;
    tarball_mock.assert_async().await;
}
