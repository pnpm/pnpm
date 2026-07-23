use super::{
    AuthHeaders, DEFAULT_REGISTRY_SCOPE, UpstreamRouteHook, base64_encode, nerf_dart,
    redact_and_sanitize, redact_url_credentials,
};
use crate::TokenHelperOutput;
use pretty_assertions::assert_eq;
use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

fn token_helper_by_uri(uri: &str, command: &[&str]) -> HashMap<String, Vec<String>> {
    std::iter::once((
        uri.to_owned(),
        command.iter().map(|part| (*part).to_owned()).collect::<Vec<String>>(),
    ))
    .collect()
}

#[test]
fn token_helper_is_executed_lazily_and_memoized() {
    // A `fn` runner can't capture, so count through a test-local static;
    // nextest runs each test in its own process, so it stays isolated.
    static CALLS: AtomicUsize = AtomicUsize::new(0);
    fn runner(command: &[String]) -> std::io::Result<TokenHelperOutput> {
        CALLS.fetch_add(1, Ordering::Relaxed);
        assert_eq!(command, ["helper", "--flag"]);
        Ok(TokenHelperOutput {
            success: true,
            stdout: "s3cr3t\n".to_owned(),
            stderr: String::new(),
        })
    }

    let auth = AuthHeaders::from_parts_with_token_helpers(
        HashMap::new(),
        HashMap::new(),
        token_helper_by_uri("//reg.com/", &["helper", "--flag"]),
        HashMap::new(),
    )
    .with_token_helper_runner(runner);

    // Constructing the map never runs the helper.
    assert_eq!(CALLS.load(Ordering::Relaxed), 0);
    assert_eq!(auth.for_url("https://reg.com/pkg"), Some("Bearer s3cr3t".to_owned()));
    // A second matching lookup reuses the memoized token.
    assert_eq!(auth.for_url("https://reg.com/other"), Some("Bearer s3cr3t".to_owned()));
    assert_eq!(CALLS.load(Ordering::Relaxed), 1);
}

#[test]
fn a_slow_token_helper_does_not_block_a_second_registry_lookup() {
    // The runner for `//slow.example/` spins until the `//fast.example/`
    // lookup has finished (or a generous deadline). If the resolution cache
    // lock were held across the subprocess, the fast lookup would block on
    // that lock, the flag would never flip, and the slow helper would run
    // for the full deadline — so the fast lookup completing promptly proves
    // the lock is released while a helper runs.
    static FAST_DONE: AtomicBool = AtomicBool::new(false);
    fn runner(command: &[String]) -> std::io::Result<TokenHelperOutput> {
        if command[0] == "slow" {
            let deadline = Instant::now() + Duration::from_secs(10);
            while !FAST_DONE.load(Ordering::Acquire) && Instant::now() < deadline {
                thread::sleep(Duration::from_millis(5));
            }
        }
        Ok(TokenHelperOutput {
            success: true,
            stdout: format!("{}-token", command[0]),
            stderr: String::new(),
        })
    }

    let mut helpers = token_helper_by_uri("//slow.example/", &["slow"]);
    helpers.extend(token_helper_by_uri("//fast.example/", &["fast"]));
    let auth = Arc::new(
        AuthHeaders::from_parts_with_token_helpers(
            HashMap::new(),
            HashMap::new(),
            helpers,
            HashMap::new(),
        )
        .with_token_helper_runner(runner),
    );

    let slow_auth = Arc::clone(&auth);
    let slow = thread::spawn(move || slow_auth.for_url("https://slow.example/pkg"));

    let started = Instant::now();
    let fast = auth.for_url("https://fast.example/pkg");
    let fast_elapsed = started.elapsed();
    FAST_DONE.store(true, Ordering::Release);

    assert_eq!(fast, Some("Bearer fast-token".to_owned()));
    assert!(fast_elapsed < Duration::from_secs(5), "fast lookup blocked for {fast_elapsed:?}");
    assert_eq!(slow.join().expect("slow thread"), Some("Bearer slow-token".to_owned()));
}

#[test]
fn a_failing_token_helper_sends_no_credential_and_does_not_fall_back() {
    fn runner(_: &[String]) -> std::io::Result<TokenHelperOutput> {
        Ok(TokenHelperOutput { success: false, stdout: String::new(), stderr: "boom".to_owned() })
    }

    // A static token sits at the host root; a failing helper at the deeper
    // path prefix must NOT fall back to it — the most-specific key owns the
    // decision, exactly as a resolved helper would.
    let auth = AuthHeaders::from_parts_with_token_helpers(
        std::iter::once(("//reg.com/".to_owned(), "Bearer root-token".to_owned())).collect(),
        HashMap::new(),
        token_helper_by_uri("//reg.com/path/", &["helper"]),
        HashMap::new(),
    )
    .with_token_helper_runner(runner);

    assert_eq!(auth.for_url("https://reg.com/path/pkg"), None);
    // A request that doesn't match the helper prefix still gets the static token.
    assert_eq!(auth.for_url("https://reg.com/pkg"), Some("Bearer root-token".to_owned()));
}

/// Records every `(url, package)` it is asked about and answers with a
/// fixed header, so a test can assert the hook — not the client
/// credentials — drove the lookup.
#[derive(Default)]
struct RecordingHook {
    calls: AtomicUsize,
    answer: Option<String>,
}

impl UpstreamRouteHook for RecordingHook {
    fn authorize(&self, _url: &str, _package: Option<&str>) -> Option<String> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        self.answer.clone()
    }
}

#[test]
fn route_hook_overrides_client_credentials() {
    let client_creds = AuthHeaders::from_map(
        std::iter::once(("//reg.com/".to_string(), "Bearer client-token".to_string())).collect(),
    );
    // Without a hook the client-forwarded token is returned.
    assert_eq!(
        client_creds.for_url("https://reg.com/pkg"),
        Some("Bearer client-token".to_string()),
    );

    let hook =
        Arc::new(RecordingHook { answer: Some("Bearer alias".to_string()), ..Default::default() });
    let hooked = client_creds.with_route_hook(Arc::clone(&hook) as Arc<dyn UpstreamRouteHook>);
    // With a hook the client token is ignored and the hook decides.
    assert_eq!(hooked.for_url("https://reg.com/pkg"), Some("Bearer alias".to_string()));
    assert_eq!(hook.calls.load(Ordering::Relaxed), 1);
}

#[test]
fn record_route_drives_the_hook_without_a_fetch() {
    let hook =
        Arc::new(RecordingHook { answer: Some("Bearer alias".to_string()), ..Default::default() });
    let hooked =
        AuthHeaders::default().with_route_hook(Arc::clone(&hook) as Arc<dyn UpstreamRouteHook>);
    // A cache-served metadata pick records its route through the hook so a
    // server footprint stays complete, discarding the selected credential
    // because no request is sent.
    hooked.record_route("https://reg.com/@scope/pkg", Some("@scope/pkg"));
    assert_eq!(hook.calls.load(Ordering::Relaxed), 1);
}

#[test]
fn record_route_is_a_noop_without_a_hook() {
    // The CLI never installs a hook: a fetch that doesn't happen needs no
    // header and there is no footprint to record into. This must not panic
    // or otherwise touch the client-forwarded credentials.
    AuthHeaders::default().record_route("https://reg.com/pkg", None);
}

#[test]
fn route_hook_suppresses_inline_url_basic_auth() {
    // A bare `AuthHeaders` would synthesize a `Basic` header from inline
    // `user:pass@`; with a hook attached the inline credential must not
    // leak through — the hook (returning None here) owns the decision.
    let hook = Arc::new(RecordingHook::default());
    let hooked = AuthHeaders::default().with_route_hook(hook as Arc<dyn UpstreamRouteHook>);
    assert_eq!(hooked.for_url("https://user:pass@reg.com/pkg"), None);
}

#[test]
fn redact_url_credentials_strips_embedded_basic_auth() {
    assert_eq!(
        redact_url_credentials(
            "Failed to fetch metadata from https://user:pass@host/pkg: timed out"
        ),
        "Failed to fetch metadata from https://host/pkg: timed out",
    );
    // user-only userinfo (no password) is stripped too.
    assert_eq!(
        redact_url_credentials("got https://token@registry.example/foo"),
        "got https://registry.example/foo",
    );
    // A raw "@" inside the password is stripped up to the last "@" in the
    // authority, so the password tail can't leak.
    assert_eq!(
        redact_url_credentials("Failed to fetch metadata from https://user:p@ss@host/pkg: 403"),
        "Failed to fetch metadata from https://host/pkg: 403",
    );
    // An "@" in the path/query (after the authority) is preserved.
    assert_eq!(
        redact_url_credentials("got https://host/path?to=a@b"),
        "got https://host/path?to=a@b",
    );
    // A credential-free URL is left untouched.
    assert_eq!(
        redact_url_credentials("Failed to fetch metadata from https://host/pkg: timed out"),
        "Failed to fetch metadata from https://host/pkg: timed out",
    );
    // A bare "://" with no preceding scheme character is not treated as a URL
    // authority, so an "@" further along is preserved.
    assert_eq!(redact_url_credentials("a :// b@c"), "a :// b@c");
}

#[test]
fn redact_and_sanitize_strips_credentials_and_control_chars() {
    assert_eq!(redact_and_sanitize("https://user:pass@host/pkg\u{7}\r\n"), "https://host/pkg");
    // A clean URL is returned unchanged.
    assert_eq!(redact_and_sanitize("https://host/pkg"), "https://host/pkg");
    // A control character inside the userinfo must not break the redaction:
    // controls are stripped first, then credentials are redacted.
    assert_eq!(redact_and_sanitize("https://user:pass\r@host/x"), "https://host/x");
}

fn build(entries: &[(&str, &str)]) -> AuthHeaders {
    AuthHeaders::from_creds_map(
        entries.iter().map(|(uri, value)| ((*uri).to_string(), (*value).to_string())),
        None,
    )
}

#[test]
fn nerf_dart_strips_protocol_query_fragment_and_filename() {
    assert_eq!(nerf_dart("https://reg.com/"), "//reg.com/");
    assert_eq!(nerf_dart("https://reg.com:8080/"), "//reg.com:8080/");
    assert_eq!(nerf_dart("https://reg.com/foo/-/foo-1.tgz"), "//reg.com/foo/-/");
    assert_eq!(
        nerf_dart("https://npm.pkg.github.com/pnpm/foo?token=x"),
        "//npm.pkg.github.com/pnpm/",
    );
    assert_eq!(nerf_dart("https://user:pw@reg.com/scoped/pkg"), "//reg.com/scoped/");
}

#[test]
fn base64_round_trip_matches_known_vectors() {
    // Sanity-check vectors from the pnpm test fixtures.
    assert_eq!(base64_encode("foobar:foobar"), "Zm9vYmFyOmZvb2Jhcg==");
    assert_eq!(base64_encode("user:pass"), "dXNlcjpwYXNz");
}

#[test]
fn matches_host_only_token() {
    let headers = build(&[("//reg.com/", "Bearer abc123")]);
    assert_eq!(headers.for_url("https://reg.com/").as_deref(), Some("Bearer abc123"));
    assert_eq!(
        headers.for_url("https://reg.com/foo/-/foo-1.0.0.tgz").as_deref(),
        Some("Bearer abc123"),
    );
    assert_eq!(headers.for_url("https://reg.io/foo/-/foo-1.0.0.tgz"), None);
}

#[test]
fn matches_path_scoped_token() {
    let headers = build(&[("//reg.com/", "Bearer abc123"), ("//reg.co/tarballs/", "Bearer xxx")]);
    assert_eq!(
        headers.for_url("https://reg.co/tarballs/foo/-/foo-1.0.0.tgz").as_deref(),
        Some("Bearer xxx"),
    );
}

#[test]
fn matches_explicit_port_token() {
    let headers = build(&[("//reg.gg:8888/", "Bearer 0000")]);
    assert_eq!(
        headers.for_url("https://reg.gg:8888/foo/-/foo-1.0.0.tgz").as_deref(),
        Some("Bearer 0000"),
    );
}

#[test]
fn default_https_port_strips_for_lookup() {
    let headers = build(&[("//reg.com/", "Bearer abc123")]);
    assert_eq!(headers.for_url("https://reg.com:443/").as_deref(), Some("Bearer abc123"));
    assert_eq!(headers.for_url("http://reg.com:80/").as_deref(), Some("Bearer abc123"));
}

#[test]
fn non_default_port_strips_for_fallback_lookup() {
    let headers = build(&[("//reg.com/", "Bearer abc123")]);
    assert_eq!(headers.for_url("https://reg.com:8080/").as_deref(), Some("Bearer abc123"));
}

#[test]
fn nerf_dart_strips_default_ports_when_keying() {
    assert_eq!(nerf_dart("https://reg.com:443/"), "//reg.com/");
    assert_eq!(nerf_dart("http://reg.com:80/"), "//reg.com/");
    assert_eq!(nerf_dart("https://reg.com:8080/"), "//reg.com:8080/");
}

#[test]
fn basic_auth_in_url_wins_over_token() {
    let headers = build(&[("//reg.com/", "Bearer abc123")]);
    let header = headers.for_url("https://user:secret@reg.com/").unwrap();
    assert_eq!(header, format!("Basic {}", base64_encode("user:secret")));
}

#[test]
fn basic_auth_works_without_settings() {
    let empty = AuthHeaders::default();
    assert_eq!(
        empty.for_url("https://user:secret@reg.io/"),
        Some(format!("Basic {}", base64_encode("user:secret"))),
    );
    assert_eq!(
        empty.for_url("https://user:@reg.io/"),
        Some(format!("Basic {}", base64_encode("user:"))),
    );
    assert_eq!(
        empty.for_url("https://user@reg.io/"),
        Some(format!("Basic {}", base64_encode("user:"))),
    );
}

#[test]
fn registry_with_pathname_matches_metadata_and_tarballs() {
    // GitHub Packages scope-registry example.
    let headers = build(&[("//npm.pkg.github.com/pnpm/", "Bearer abc123")]);
    assert_eq!(
        headers.for_url("https://npm.pkg.github.com/pnpm").as_deref(),
        Some("Bearer abc123"),
    );
    assert_eq!(
        headers.for_url("https://npm.pkg.github.com/pnpm/").as_deref(),
        Some("Bearer abc123"),
    );
    assert_eq!(
        headers.for_url("https://npm.pkg.github.com/pnpm/foo/-/foo-1.0.0.tgz").as_deref(),
        Some("Bearer abc123"),
    );
}

#[test]
fn package_scope_auth_wins_over_registry_auth() {
    let headers = build(&[
        ("//npm.pkg.github.com/", "Bearer registry-token"),
        ("//npm.pkg.github.com/:@orgA", "Bearer org-a-token"),
        ("//npm.pkg.github.com/:@orgB", "Bearer org-b-token"),
    ]);
    assert_eq!(
        headers.for_url_with_package("https://npm.pkg.github.com/", Some("@orgA/pkg")).as_deref(),
        Some("Bearer org-a-token"),
    );
    assert_eq!(
        headers.for_url_with_package("https://npm.pkg.github.com/", Some("@orgB/pkg")).as_deref(),
        Some("Bearer org-b-token"),
    );
    assert_eq!(
        headers.for_url_with_package("https://npm.pkg.github.com/", Some("@orgC/pkg")).as_deref(),
        Some("Bearer registry-token"),
    );
    assert_eq!(
        headers.for_url_with_package("https://npm.pkg.github.com/", Some("pkg")).as_deref(),
        Some("Bearer registry-token"),
    );
    assert_eq!(
        headers
            .for_url_with_package("https://npm.pkg.github.com/download/pkg.tgz", Some("@orgA/pkg"))
            .as_deref(),
        Some("Bearer org-a-token"),
    );
}

#[test]
fn slash_package_scope_auth_wins_over_registry_auth() {
    let headers = build(&[
        ("//npm.pkg.github.com/", "Bearer registry-token"),
        ("//npm.pkg.github.com/@orgA", "Bearer org-a-token"),
        ("//npm.pkg.github.com/@orgB/", "Bearer org-b-token"),
    ]);
    assert_eq!(
        headers.for_url_with_package("https://npm.pkg.github.com/", Some("@orgA/pkg")).as_deref(),
        Some("Bearer org-a-token"),
    );
    assert_eq!(
        headers.for_url_with_package("https://npm.pkg.github.com/", Some("@orgB/pkg")).as_deref(),
        Some("Bearer org-b-token"),
    );
}

#[test]
fn package_scope_auth_keeps_registry_path() {
    let headers = build(&[
        ("//reg.com/npm/", "Bearer registry-token"),
        ("//reg.com/npm/:@orgA", "Bearer org-a-token"),
    ]);
    assert_eq!(
        headers.for_url_with_package("https://reg.com/npm/", Some("@orgA/pkg")).as_deref(),
        Some("Bearer org-a-token"),
    );
    assert_eq!(
        headers
            .for_url_with_package("https://reg.com/npm/pkg/-/pkg-1.0.0.tgz", Some("@orgA/pkg"))
            .as_deref(),
        Some("Bearer org-a-token"),
    );
    assert_eq!(
        headers.for_url_with_package("https://reg.com/npm/", Some("@orgB/pkg")).as_deref(),
        Some("Bearer registry-token"),
    );
}

#[test]
fn entries_round_trip_package_scope_auth() {
    let headers = build(&[
        ("//npm.pkg.github.com/", "Bearer registry-token"),
        ("//npm.pkg.github.com/:@orgA", "Bearer org-a-token"),
        ("//reg.com/npm/:@orgA", "Bearer org-a-path-token"),
    ]);
    let by_scope = headers.to_by_scope();
    assert_eq!(
        by_scope
            .get("//npm.pkg.github.com/")
            .and_then(|scope_headers| scope_headers.get(DEFAULT_REGISTRY_SCOPE))
            .map(String::as_str),
        Some("Bearer registry-token"),
    );
    assert_eq!(
        by_scope
            .get("//npm.pkg.github.com/")
            .and_then(|scope_headers| scope_headers.get("@orgA"))
            .map(String::as_str),
        Some("Bearer org-a-token"),
    );
    assert_eq!(
        by_scope
            .get("//reg.com/npm/")
            .and_then(|scope_headers| scope_headers.get("@orgA"))
            .map(String::as_str),
        Some("Bearer org-a-path-token"),
    );

    let round_tripped = AuthHeaders::from_by_scope(by_scope);
    assert_eq!(
        round_tripped
            .for_url_with_package("https://reg.com/npm/pkg/-/pkg-1.0.0.tgz", Some("@orgA/pkg"))
            .as_deref(),
        Some("Bearer org-a-path-token"),
    );
}

#[test]
fn basic_auth_in_url_wins_over_package_scope_auth() {
    let headers = build(&[("//reg.com/:@orgA", "Bearer org-a-token")]);
    let header =
        headers.for_url_with_package("https://user:secret@reg.com/", Some("@orgA/pkg")).unwrap();
    assert_eq!(header, format!("Basic {}", base64_encode("user:secret")));
}

#[test]
fn default_registry_creds_apply_to_npmjs_when_unspecified() {
    let headers = AuthHeaders::from_creds_map(
        [(String::new(), "Bearer default-token".to_owned())],
        Some("https://registry.npmjs.org/"),
    );
    assert_eq!(
        headers.for_url("https://registry.npmjs.org/").as_deref(),
        Some("Bearer default-token"),
    );
    assert_eq!(
        headers.for_url("https://registry.npmjs.org/foo/-/foo-1.0.0.tgz").as_deref(),
        Some("Bearer default-token"),
    );
}

#[test]
fn registry_with_pathname_matches_with_explicit_port() {
    let headers = build(&[("//custom.domain.com/artifactory/api/npm/npm-virtual/", "Bearer xyz")]);
    assert_eq!(
        headers
            .for_url("https://custom.domain.com:443/artifactory/api/npm/npm-virtual/")
            .as_deref(),
        Some("Bearer xyz"),
    );
    assert_eq!(
        headers
            .for_url(
                "https://custom.domain.com:443/artifactory/api/npm/npm-virtual/@platform/device-utils/-/@platform/device-utils-1.0.0.tgz",
            )
            .as_deref(),
        Some("Bearer xyz"),
    );
    assert_eq!(
        headers.for_url("https://custom.domain.com:443/artifactory/api/npm/").as_deref(),
        None,
    );
}

#[test]
fn returns_none_for_unmatched_url_in_empty_map() {
    assert_eq!(AuthHeaders::default().for_url("http://reg.com"), None);
}

/// `from_creds_map` processes per-URI entries first, then unconditionally
/// overwrites the default-registry slot with the default-creds header.
/// When a `.npmrc` carries both `_authToken=A` (default) and
/// `//registry.npmjs.org/:_authToken=B` (per-URI for the default
/// registry), the *default* (A) must win on the default registry.
/// Without the two-phase build in `from_creds_map`, pacquet's `HashMap`
/// iteration would let either value win non-deterministically.
#[test]
fn default_creds_win_over_per_uri_on_default_registry() {
    let headers = AuthHeaders::from_creds_map(
        [
            ("//registry.npmjs.org/".to_owned(), "Bearer per-uri".to_owned()),
            (String::new(), "Bearer default".to_owned()),
        ],
        Some("https://registry.npmjs.org/"),
    );
    assert_eq!(
        headers.for_url("https://registry.npmjs.org/foo").as_deref(),
        Some("Bearer default"),
    );
}

/// Specifically exercises the trailing-slash-append branch in
/// [`AuthHeaders::for_url`]: the URL ends without a `/` *and*
/// names a path segment (`/scope`). Without the append,
/// [`nerf_dart`] would drop the segment and miss the token; with
/// it, the lookup walks `//reg.com/scope/`. Removing the append
/// branch makes this test fail. Kept as a focused single-assertion
/// case for the slash-append branch even though
/// [`registry_with_pathname_matches_metadata_and_tarballs`]'s first
/// assertion (`https://npm.pkg.github.com/pnpm`) also exercises it.
#[test]
fn slash_append_branch_lets_path_segment_match() {
    let headers = build(&[("//reg.com/scope/", "Bearer scoped")]);
    assert_eq!(headers.for_url("https://reg.com/scope").as_deref(), Some("Bearer scoped"));
}

/// Hits the `None => return String::new()` branch of [`nerf_dart`]
/// (and the `?` short-circuit in [`ParsedUrl::parse`]).
#[test]
fn nerf_dart_returns_empty_for_malformed_url() {
    assert_eq!(nerf_dart("not-a-url"), "");
    assert_eq!(nerf_dart(""), "");
    let headers = build(&[("//reg.com/", "Bearer abc123")]);
    assert_eq!(headers.for_url("not-a-url"), None);
}

/// Hits the no-path-separator branch (`None => (rest, "")`) inside
/// [`ParsedUrl::parse`]: the URL has no `/` after the authority.
/// The parsed `path` is an empty string, so [`nerf_dart`] should
/// produce `//host/`.
#[test]
fn nerf_dart_handles_url_with_no_path_separator() {
    assert_eq!(nerf_dart("https://reg.com"), "//reg.com/");
    assert_eq!(nerf_dart("https://reg.com:8080"), "//reg.com:8080/");
}

/// Hits the `user.is_empty() && pass.is_empty()` short-circuit in
/// [`ParsedUrl::basic_auth_header`]: a URL whose authority parses
/// as `@host` must not produce a `Basic ` header.
#[test]
fn empty_user_info_returns_no_basic_header() {
    let empty = AuthHeaders::default();
    assert_eq!(empty.for_url("https://@reg.com/"), None);
}
