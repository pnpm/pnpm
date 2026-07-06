use super::{
    DistHashes, PackedPkg, PublishHttpError, PublishNetwork, PublishPackedPkgError,
    PublishPackedPkgOptions, build_publish_document, clean_version, is_otp_challenge,
    parse_otp_challenge, publish_packed_pkg, publish_with_otp_handling, put_publish,
    registry_for_display, web_auth_fetch_options,
};
use crate::{
    capabilities::{Clock, EnvVar, OidcFetch, OidcFetchError, OidcRequest, OidcResponse},
    oidc::OidcHttpOptions,
    provenance_gen::{ProvenanceGenError, SignProvenance, SignedProvenance},
    publish_options::{CreatePublishOptionsError, PublishUnsupportedRegistryProtocolError},
    registry_config_keys::parse_supported_registry_url,
};
use pacquet_network::{AuthHeaders, ThrottledClient};
use pacquet_network_web_auth::{
    Host as WebAuthHost, OtpChallenge, OtpError, WebAuthFetchOptions, WithOtpError,
};
use pacquet_network_web_auth_testing::{
    InputResponse, SleepBehavior, ok_202, ok_token, web_auth_fake,
};
use pacquet_reporter::SilentReporter;
use pretty_assertions::assert_eq;
use serde_json::{Value, json};

/// A `WebAuthFetchOptions` the success paths never reach: when the PUT
/// resolves without a 401 challenge the web-auth poller is never invoked, so
/// the timeout/retry knobs are irrelevant.
fn unused_fetch_options() -> WebAuthFetchOptions {
    WebAuthFetchOptions { timeout: None, retry: None }
}

fn body() -> bytes::Bytes {
    bytes::Bytes::from_static(b"{}")
}

fn registry() -> crate::registry_config_keys::NormalizedRegistryUrl {
    parse_supported_registry_url("https://registry.example/").unwrap().normalized_url
}

fn hashes() -> DistHashes<'static> {
    DistHashes { integrity: "sha512-deadbeef", shasum: "abc123" }
}

#[test]
fn cleans_versions() {
    assert_eq!(clean_version("=v1.2.3").unwrap(), "1.2.3");
    assert_eq!(clean_version("  1.0.0 ").unwrap(), "1.0.0");
    assert!(clean_version("not-a-version").is_err());
}

#[test]
fn clean_version_drops_build_metadata_keeps_prerelease() {
    // `semver.clean` returns SemVer.version, which excludes build metadata.
    assert_eq!(clean_version("1.2.3+build.5").unwrap(), "1.2.3");
    assert_eq!(clean_version("1.2.3-rc.1+build").unwrap(), "1.2.3-rc.1");
}

#[test]
fn builds_document_with_dist_and_attachment() {
    let manifest = json!({ "name": "@scope/pkg", "version": "1.0.0", "description": "hi" });
    let document =
        build_publish_document(&manifest, b"tarball", &registry(), None, "latest", &hashes())
            .unwrap();

    assert_eq!(document["name"], "@scope/pkg");
    assert_eq!(document["dist-tags"]["latest"], "1.0.0");
    let version = &document["versions"]["1.0.0"];
    assert_eq!(version["_id"], "@scope/pkg@1.0.0");
    assert_eq!(version["dist"]["integrity"], "sha512-deadbeef");
    assert_eq!(version["dist"]["shasum"], "abc123");
    // The tarball URL is stored as http:// even for an https registry.
    assert_eq!(
        version["dist"]["tarball"],
        "http://registry.example/@scope/pkg/-/@scope/pkg-1.0.0.tgz",
    );
    let attachments = document["_attachments"].as_object().unwrap();
    assert!(attachments.contains_key("@scope/pkg-1.0.0.tgz"));
    assert_eq!(document["access"], Value::Null);
}

#[test]
fn detects_otp_challenge_by_header_token_or_body() {
    assert!(is_otp_challenge(Some("ipaddress, otp"), ""));
    assert!(is_otp_challenge(Some("OTP"), ""));
    assert!(is_otp_challenge(None, "you must provide a one-time pass"));
    // A bare substring in another token must not over-match.
    assert!(!is_otp_challenge(Some(r#"Basic realm="notop""#), "denied"));
    assert!(!is_otp_challenge(None, "forbidden"));
}

#[test]
fn manifest_level_tag_overrides_the_default() {
    let manifest = json!({ "name": "pkg", "version": "1.0.0", "tag": "next" });
    let document =
        build_publish_document(&manifest, b"x", &registry(), None, "latest", &hashes()).unwrap();
    assert_eq!(document["dist-tags"]["next"], "1.0.0");
    assert!(document["dist-tags"].get("latest").is_none());
}

#[test]
fn rejects_restricted_access_for_unscoped_package() {
    let manifest = json!({ "name": "pkg", "version": "1.0.0" });
    let err = build_publish_document(
        &manifest,
        b"x",
        &registry(),
        Some(super::Access::Restricted),
        "latest",
        &hashes(),
    )
    .unwrap_err();
    assert!(matches!(err, super::PublishPackedPkgError::UnscopedRestricted { .. }));
}

#[test]
fn rejects_private_package() {
    let manifest = json!({ "name": "pkg", "version": "1.0.0", "private": true });
    let err = build_publish_document(&manifest, b"x", &registry(), None, "latest", &hashes())
        .unwrap_err();
    assert!(matches!(err, super::PublishPackedPkgError::Private));
}

#[test]
fn parse_otp_challenge_extracts_auth_and_done_urls() {
    let challenge = parse_otp_challenge(
        r#"{"authUrl":"https://r/auth/abc","doneUrl":"https://r/auth/abc/done"}"#,
    );
    let body = challenge.body.expect("web-auth challenge carries a body");
    assert_eq!(body.auth_url.as_deref(), Some("https://r/auth/abc"));
    assert_eq!(body.done_url.as_deref(), Some("https://r/auth/abc/done"));
}

#[test]
fn parse_otp_challenge_yields_no_urls_for_a_plain_otp_body() {
    // A classic (non-web-auth) OTP challenge has no JSON `authUrl`/`doneUrl`,
    // so both fall back to `None` rather than erroring.
    let challenge = parse_otp_challenge("you must provide a one-time pass");
    let body = challenge.body.expect("body is always present");
    assert_eq!(body.auth_url, None);
    assert_eq!(body.done_url, None);
}

#[test]
fn parse_otp_challenge_reads_each_url_independently() {
    let challenge = parse_otp_challenge(r#"{"authUrl":"https://r/auth/abc"}"#);
    let body = challenge.body.expect("body is always present");
    assert_eq!(body.auth_url.as_deref(), Some("https://r/auth/abc"));
    assert_eq!(body.done_url, None);
}

#[tokio::test]
async fn put_publish_returns_an_ok_response_on_success() {
    let mut server = mockito::Server::new_async().await;
    let mock =
        server.mock("PUT", "/pkg").with_status(200).with_body("").expect(1).create_async().await;
    let client = ThrottledClient::default();
    let url = format!("{}/pkg", server.url());

    let response = put_publish(&client, &url, Some("Bearer t"), "publish", body(), None, false)
        .await
        .expect("the PUT completes");

    assert!(response.ok);
    assert_eq!(response.status, 200);
    assert_eq!(response.status_text, "OK");
    assert_eq!(response.stage_id, None);
    mock.assert_async().await;
}

#[tokio::test]
async fn put_publish_reports_a_non_success_status_without_erroring() {
    let mut server = mockito::Server::new_async().await;
    server.mock("PUT", "/pkg").with_status(500).with_body("boom").create_async().await;
    let client = ThrottledClient::default();
    let url = format!("{}/pkg", server.url());

    // A 5xx is a completed response (`ok: false`), not a transport error — the
    // caller inspects `ok` and raises `FailedToPublishError`, matching pnpm.
    let response = put_publish(&client, &url, None, "publish", body(), None, false)
        .await
        .expect("the PUT completes");
    assert!(!response.ok);
    assert_eq!(response.status, 500);
}

#[tokio::test]
async fn put_publish_maps_a_www_authenticate_otp_to_a_challenge() {
    let mut server = mockito::Server::new_async().await;
    server
        .mock("PUT", "/pkg")
        .with_status(401)
        .with_header("www-authenticate", "otp")
        .with_body("")
        .create_async()
        .await;
    let client = ThrottledClient::default();
    let url = format!("{}/pkg", server.url());

    let err = put_publish(&client, &url, None, "publish", body(), None, false)
        .await
        .expect_err("a 401 OTP challenge is an error the OTP flow handles");
    assert!(matches!(err, PublishHttpError::Otp { .. }));
}

#[tokio::test]
async fn put_publish_maps_a_one_time_pass_body_to_a_web_auth_challenge() {
    let mut server = mockito::Server::new_async().await;
    let challenge_body = r#"{"error":"one-time pass required","authUrl":"https://r/auth/abc","doneUrl":"https://r/auth/abc/done"}"#;
    server.mock("PUT", "/pkg").with_status(401).with_body(challenge_body).create_async().await;
    let client = ThrottledClient::default();
    let url = format!("{}/pkg", server.url());

    let err = put_publish(&client, &url, None, "publish", body(), None, false)
        .await
        .expect_err("a one-time-pass body is an OTP challenge");
    let PublishHttpError::Otp { challenge } = err else {
        panic!("expected an OTP challenge, got {err:?}");
    };
    let challenge_body = challenge.body.expect("web-auth challenge carries a body");
    assert_eq!(challenge_body.auth_url.as_deref(), Some("https://r/auth/abc"));
    assert_eq!(challenge_body.done_url.as_deref(), Some("https://r/auth/abc/done"));
}

#[tokio::test]
async fn put_publish_extracts_a_stage_id_only_for_a_staged_publish() {
    let mut server = mockito::Server::new_async().await;
    server
        .mock("PUT", "/pkg")
        .with_status(200)
        .with_body(r#"{"stageId":"stage-1"}"#)
        .expect(2)
        .create_async()
        .await;
    let client = ThrottledClient::default();
    let url = format!("{}/pkg", server.url());

    let staged = put_publish(&client, &url, None, "publish", body(), None, true)
        .await
        .expect("the PUT completes");
    assert_eq!(staged.stage_id.as_deref(), Some("stage-1"));

    // Without `is_stage` the same body must not yield a stage id.
    let unstaged = put_publish(&client, &url, None, "publish", body(), None, false)
        .await
        .expect("the PUT completes");
    assert_eq!(unstaged.stage_id, None);
}

#[tokio::test]
async fn put_publish_sends_the_command_auth_and_otp_headers() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("PUT", "/pkg")
        .match_header("content-type", "application/json")
        .match_header("npm-auth-type", "web")
        .match_header("npm-command", "publish")
        .match_header("authorization", "Bearer tok")
        .match_header("npm-otp", "123456")
        .with_status(200)
        .with_body("")
        .expect(1)
        .create_async()
        .await;
    let client = ThrottledClient::default();
    let url = format!("{}/pkg", server.url());

    put_publish(&client, &url, Some("Bearer tok"), "publish", body(), Some("123456"), false)
        .await
        .expect("the PUT completes");
    mock.assert_async().await;
}

#[tokio::test]
async fn put_publish_omits_auth_and_otp_headers_when_absent() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("PUT", "/pkg")
        .match_header("authorization", mockito::Matcher::Missing)
        .match_header("npm-otp", mockito::Matcher::Missing)
        .with_status(200)
        .with_body("")
        .expect(1)
        .create_async()
        .await;
    let client = ThrottledClient::default();
    let url = format!("{}/pkg", server.url());

    put_publish(&client, &url, None, "publish", body(), None, false)
        .await
        .expect("the PUT completes");
    mock.assert_async().await;
}

#[tokio::test]
async fn put_publish_classifies_a_connection_failure_as_a_transport_error() {
    // Port 1 has no listener, so the request never gets a response.
    let client = ThrottledClient::default();
    let err = put_publish(&client, "http://127.0.0.1:1/pkg", None, "publish", body(), None, false)
        .await
        .expect_err("a refused connection is a transport failure");
    assert!(matches!(err, PublishHttpError::Transport { .. }));
}

#[tokio::test]
async fn publish_with_otp_handling_returns_the_response_when_no_otp_is_required() {
    let mut server = mockito::Server::new_async().await;
    let mock =
        server.mock("PUT", "/pkg").with_status(200).with_body("").expect(1).create_async().await;
    let client = ThrottledClient::default();
    let url = format!("{}/pkg", server.url());

    let response = publish_with_otp_handling::<WebAuthHost, SilentReporter>(
        &client,
        &url,
        None,
        "publish",
        body(),
        None,
        false,
        unused_fetch_options(),
    )
    .await
    .expect("the publish succeeds without an OTP challenge");
    assert!(response.ok);
    mock.assert_async().await;
}

/// The classic OTP flow, driven end-to-end through the publish HTTP layer:
/// prompt for an OTP on the challenge and retry. The first PUT
/// (no `npm-otp`) gets a 401 OTP challenge, the fake host prompts and returns
/// the code, and the retry PUT — distinguished by the `npm-otp` header it now
/// carries — succeeds. Exercises the `put_publish` ↔ `with_otp_handling` seam
/// a mocked operation cannot.
#[tokio::test]
async fn classic_otp_flow_prompts_then_retries_with_the_code() {
    web_auth_fake!();
    reset();
    set_input(InputResponse::Value(Some("654321".to_owned())));
    let mut server = mockito::Server::new_async().await;
    let challenge = server
        .mock("PUT", "/pkg")
        .match_header("npm-otp", mockito::Matcher::Missing)
        .with_status(401)
        .with_header("www-authenticate", "otp")
        .with_body("")
        .expect(1)
        .create_async()
        .await;
    let retry = server
        .mock("PUT", "/pkg")
        .match_header("npm-otp", "654321")
        .with_status(200)
        .with_body("")
        .expect(1)
        .create_async()
        .await;
    let client = ThrottledClient::default();
    let url = format!("{}/pkg", server.url());

    let response = publish_with_otp_handling::<FakeHost, RecordingReporter>(
        &client,
        &url,
        None,
        "publish",
        body(),
        None,
        false,
        WebAuthFetchOptions::default(),
    )
    .await
    .expect("the retry publish succeeds");
    assert!(response.ok);
    challenge.assert_async().await;
    retry.assert_async().await;
}

/// When the registry rejects the retry with another OTP challenge the flow
/// gives up (a second-challenge error) rather than prompting a second time.
#[tokio::test]
async fn classic_otp_flow_second_challenge_is_an_error() {
    web_auth_fake!();
    reset();
    set_input(InputResponse::Value(Some("123456".to_owned())));
    let mut server = mockito::Server::new_async().await;
    server
        .mock("PUT", "/pkg")
        .match_header("npm-otp", mockito::Matcher::Missing)
        .with_status(401)
        .with_header("www-authenticate", "otp")
        .with_body("")
        .create_async()
        .await;
    server
        .mock("PUT", "/pkg")
        .match_header("npm-otp", mockito::Matcher::Any)
        .with_status(401)
        .with_header("www-authenticate", "otp")
        .with_body("")
        .create_async()
        .await;
    let client = ThrottledClient::default();
    let url = format!("{}/pkg", server.url());

    let err = publish_with_otp_handling::<FakeHost, RecordingReporter>(
        &client,
        &url,
        None,
        "publish",
        body(),
        None,
        false,
        WebAuthFetchOptions::default(),
    )
    .await
    .expect_err("a second challenge aborts the publish");
    assert!(matches!(err, WithOtpError::SecondChallenge(_)), "got {err:?}");
}

/// A non-TTY session cannot answer the challenge, so the flow fails fast
/// (a non-interactive error) without a retry PUT.
#[tokio::test]
async fn non_interactive_terminal_rejects_the_otp_challenge() {
    web_auth_fake!();
    reset();
    set_stdin_tty(false);
    let mut server = mockito::Server::new_async().await;
    server
        .mock("PUT", "/pkg")
        .with_status(401)
        .with_header("www-authenticate", "otp")
        .with_body("")
        .create_async()
        .await;
    let client = ThrottledClient::default();
    let url = format!("{}/pkg", server.url());

    let err = publish_with_otp_handling::<FakeHost, RecordingReporter>(
        &client,
        &url,
        None,
        "publish",
        body(),
        None,
        false,
        WebAuthFetchOptions::default(),
    )
    .await
    .expect_err("a non-interactive terminal cannot answer the challenge");
    assert!(matches!(err, WithOtpError::NonInteractive(_)), "got {err:?}");
}

/// The web-auth flow: the 401 carries `authUrl`/`doneUrl`, the fake host
/// polls (202, 202, token), and the retry PUT carries the web token. The auth
/// URL is surfaced to the user.
#[tokio::test]
async fn web_auth_flow_polls_then_retries_with_the_web_token() {
    web_auth_fake!();
    reset();
    let mut fetches = 0;
    set_fetch(Box::new(move || {
        fetches += 1;
        Ok(if fetches < 3 { ok_202() } else { ok_token("web-token-123") })
    }));
    let mut server = mockito::Server::new_async().await;
    let challenge_body = r#"{"error":"one-time pass required","authUrl":"https://registry.npmjs.org/auth/abc","doneUrl":"https://registry.npmjs.org/auth/abc/done"}"#;
    let challenge = server
        .mock("PUT", "/pkg")
        .match_header("npm-otp", mockito::Matcher::Missing)
        .with_status(401)
        .with_header("www-authenticate", "otp")
        .with_body(challenge_body)
        .expect(1)
        .create_async()
        .await;
    let retry = server
        .mock("PUT", "/pkg")
        .match_header("npm-otp", "web-token-123")
        .with_status(200)
        .with_body("")
        .expect(1)
        .create_async()
        .await;
    let client = ThrottledClient::default();
    let url = format!("{}/pkg", server.url());

    let response = publish_with_otp_handling::<FakeHost, RecordingReporter>(
        &client,
        &url,
        None,
        "publish",
        body(),
        None,
        false,
        WebAuthFetchOptions::default(),
    )
    .await
    .expect("the web-auth retry succeeds");
    assert!(response.ok);
    assert!(
        infos().iter().any(|message| message.contains("https://registry.npmjs.org/auth/abc")),
        "the auth URL should be surfaced, got {:?}",
        infos(),
    );
    challenge.assert_async().await;
    retry.assert_async().await;
}

/// The poll never completes and the fake clock jumps past the deadline, so
/// the flow times out without ever retrying the PUT.
#[tokio::test]
async fn web_auth_flow_times_out_when_the_poll_never_completes() {
    web_auth_fake!();
    reset();
    set_fetch(Box::new(|| Ok(ok_202())));
    set_sleep_behavior(SleepBehavior::AdvanceByFixed(6 * 60 * 1000));
    let mut server = mockito::Server::new_async().await;
    let challenge_body = r#"{"error":"one-time pass required","authUrl":"https://registry.npmjs.org/auth/abc","doneUrl":"https://registry.npmjs.org/auth/abc/done"}"#;
    server
        .mock("PUT", "/pkg")
        .with_status(401)
        .with_header("www-authenticate", "otp")
        .with_body(challenge_body)
        .create_async()
        .await;
    let client = ThrottledClient::default();
    let url = format!("{}/pkg", server.url());

    let err = publish_with_otp_handling::<FakeHost, RecordingReporter>(
        &client,
        &url,
        None,
        "publish",
        body(),
        None,
        false,
        WebAuthFetchOptions::default(),
    )
    .await
    .expect_err("the web-auth poll times out");
    assert!(matches!(err, WithOtpError::Timeout(_)), "got {err:?}");
}

#[test]
fn web_auth_fetch_options_maps_the_retry_and_timeout_knobs() {
    let http = OidcHttpOptions {
        fetch_retries: Some(5),
        fetch_retry_factor: Some(2.0),
        fetch_retry_maxtimeout: Some(60_000),
        fetch_retry_mintimeout: Some(1_000),
        fetch_timeout: Some(30_000),
    };
    let options = web_auth_fetch_options(&http);
    assert_eq!(options.timeout, Some(30_000));
    let retry = options.retry.expect("the publish flow always sets retry options");
    assert_eq!(retry.retries, Some(5));
    assert_eq!(retry.factor, Some(2.0));
    assert_eq!(retry.max_timeout, Some(60_000));
    assert_eq!(retry.min_timeout, Some(1_000));
    assert_eq!(retry.randomize, None);
}

#[test]
fn publish_http_error_surfaces_an_otp_challenge_but_not_a_transport_failure() {
    let otp = PublishHttpError::Otp { challenge: OtpChallenge::default() };
    assert!(otp.as_otp_challenge().is_some());

    let transport = PublishHttpError::Transport { reason: "connection refused".to_owned() };
    assert!(transport.as_otp_challenge().is_none());
}

#[test]
fn publish_packed_pkg_error_wraps_option_and_otp_failures() {
    let from_options = PublishPackedPkgError::from(CreatePublishOptionsError::from(
        PublishUnsupportedRegistryProtocolError { registry_url: "ftp://example.com/".to_owned() },
    ));
    assert!(matches!(from_options, PublishPackedPkgError::CreateOptions(_)));

    let from_otp =
        PublishPackedPkgError::from(WithOtpError::Operation(PublishHttpError::Transport {
            reason: "connection refused".to_owned(),
        }));
    assert!(matches!(from_otp, PublishPackedPkgError::Otp(_)));
}

/// A host outside CI: no `NPM_ID_TOKEN`, not GitHub Actions, so the OIDC probe
/// in `create_publish_options` resolves to "not applicable" without touching
/// the network — and the token exchange (clock) and any registry fetch stay
/// unreachable.
struct OfflineSys;
impl Clock for OfflineSys {
    fn now_ms() -> u64 {
        unreachable!("no OIDC token exchange outside CI")
    }
}
impl EnvVar for OfflineSys {
    fn var(_: &str) -> Option<String> {
        None
    }
}
impl OidcFetch for OfflineSys {
    async fn fetch(_: OidcRequest<'_>) -> Result<OidcResponse, OidcFetchError> {
        unreachable!("a dry run makes no network request")
    }
}
impl SignProvenance for OfflineSys {
    async fn sign_statement(_: &str, _: &[u8]) -> Result<SignedProvenance, ProvenanceGenError> {
        unreachable!("a dry run never signs provenance")
    }
}

/// A dry run resolves options and hashes the tarball, then returns the summary
/// before the registry PUT. With an offline host no network request is made.
#[tokio::test]
async fn publish_packed_pkg_dry_run_returns_the_summary_without_publishing() {
    let manifest = json!({ "name": "@scope/pkg", "version": "1.2.3" });
    let tarball = b"tarball-bytes";
    let contents = ["package.json".to_owned()];
    let pkg = PackedPkg {
        published_manifest: &manifest,
        tarball_data: tarball,
        tarball_path: "@scope/pkg-1.2.3.tgz",
        contents: &contents,
        unpacked_size: 42,
    };
    let opts = PublishPackedPkgOptions {
        default_registry: "https://registry.example/".to_owned(),
        scoped_registries: std::collections::BTreeMap::new(),
        access: None,
        tag: "latest".to_owned(),
        otp: None,
        provenance: None,
        dry_run: true,
        stage: false,
        http: OidcHttpOptions::default(),
    };
    let client = ThrottledClient::default();
    let auth_headers = AuthHeaders::default();
    let network = PublishNetwork { client: &client, auth_headers: &auth_headers };

    let summary = publish_packed_pkg::<OfflineSys, SilentReporter>(&pkg, &opts, &network)
        .await
        .expect("a dry run succeeds offline");

    assert_eq!(summary.id, "@scope/pkg@1.2.3");
    assert_eq!(summary.name, "@scope/pkg");
    assert_eq!(summary.version, "1.2.3");
    assert_eq!(summary.size, tarball.len() as u64);
    assert_eq!(summary.unpacked_size, 42);
    assert_eq!(summary.stage_id, None);
}

/// A GitHub-Actions host wired for the whole `--provenance` publish flow: an
/// `NPM_ID_TOKEN` short-circuits the OIDC id-token fetch, the `fetch` answers
/// both the auth-token exchange and the sigstore-audience token request, the
/// build-statement env is present, and the signer returns a canned bundle so no
/// real sigstore call is made. The registry `PUT` itself still goes over the
/// wire to the mocked server.
struct ProvenanceSys;
impl Clock for ProvenanceSys {
    fn now_ms() -> u64 {
        0
    }
}
impl EnvVar for ProvenanceSys {
    fn var(name: &str) -> Option<String> {
        let value = match name {
            "GITHUB_ACTIONS" => "true",
            "NPM_ID_TOKEN" => "npm-id-token",
            "ACTIONS_ID_TOKEN_REQUEST_TOKEN" => "request-token",
            "ACTIONS_ID_TOKEN_REQUEST_URL" => "https://github.example/token",
            "GITHUB_SERVER_URL" => "https://github.com",
            "GITHUB_REPOSITORY" => "pnpm/pnpm",
            "GITHUB_WORKFLOW_REF" => "pnpm/pnpm/.github/workflows/release.yml@refs/heads/main",
            "GITHUB_REF" => "refs/heads/main",
            "GITHUB_SHA" => "abc123",
            "RUNNER_ENVIRONMENT" => "github-hosted",
            _ => return None,
        };
        Some(value.to_owned())
    }
}
impl OidcFetch for ProvenanceSys {
    async fn fetch(request: OidcRequest<'_>) -> Result<OidcResponse, OidcFetchError> {
        let body = if request.url.contains("audience=sigstore") {
            r#"{"value":"sigstore-token"}"#
        } else if request.url.contains("/oidc/token/exchange/") {
            r#"{"token":"registry-token"}"#
        } else {
            unreachable!("unexpected OIDC request: {}", request.url)
        };
        Ok(OidcResponse { ok: true, status: 200, body: body.to_owned() })
    }
}
impl SignProvenance for ProvenanceSys {
    async fn sign_statement(_: &str, _: &[u8]) -> Result<SignedProvenance, ProvenanceGenError> {
        Ok(SignedProvenance {
            media_type: "application/vnd.dev.sigstore.bundle.v0.3+json".to_owned(),
            data: "signed-bundle-json".to_owned(),
        })
    }
}

/// The `--provenance` flow: `publish_packed_pkg` runs the *real*
/// `generate_provenance` (only the signer is faked) and splices the signed
/// `.sigstore` bundle into the document's `_attachments` before the `PUT`.
#[tokio::test]
async fn publish_packed_pkg_attaches_signed_provenance_to_the_document() {
    let mut server = mockito::Server::new_async().await;
    let manifest = json!({ "name": "pkg", "version": "1.0.0" });
    let tarball = b"tarball-bytes";
    let pkg = PackedPkg {
        published_manifest: &manifest,
        tarball_data: tarball,
        tarball_path: "pkg-1.0.0.tgz",
        contents: &[],
        unpacked_size: 0,
    };
    let opts = PublishPackedPkgOptions {
        default_registry: format!("{}/", server.url()),
        scoped_registries: std::collections::BTreeMap::new(),
        access: None,
        tag: "latest".to_owned(),
        otp: None,
        provenance: Some(true),
        dry_run: false,
        stage: false,
        http: OidcHttpOptions::default(),
    };
    let mock = server
        .mock("PUT", "/pkg")
        .match_body(mockito::Matcher::PartialJsonString(
            r#"{"_attachments":{"pkg-1.0.0.sigstore":{"content_type":"application/vnd.dev.sigstore.bundle.v0.3+json","data":"signed-bundle-json"}}}"#
                .to_owned(),
        ))
        .with_status(200)
        .with_body("{}")
        .expect(1)
        .create_async()
        .await;
    let client = ThrottledClient::default();
    let auth_headers = AuthHeaders::default();
    let network = PublishNetwork { client: &client, auth_headers: &auth_headers };

    let summary = publish_packed_pkg::<ProvenanceSys, SilentReporter>(&pkg, &opts, &network)
        .await
        .expect("a provenance publish succeeds");

    assert_eq!(summary.name, "pkg");
    mock.assert_async().await;
}

#[test]
fn registry_for_display_strips_userinfo_and_leaves_a_plain_registry_untouched() {
    let plain = parse_supported_registry_url("https://registry.npmjs.org/")
        .expect("a supported registry")
        .normalized_url;
    assert_eq!(registry_for_display(&plain), "https://registry.npmjs.org/");

    let with_credentials =
        parse_supported_registry_url("https://user:secret@registry.example.com/")
            .expect("a supported registry")
            .normalized_url;
    assert_eq!(registry_for_display(&with_credentials), "https://registry.example.com/");
}
