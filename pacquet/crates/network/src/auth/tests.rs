use super::{AuthHeaders, base64_encode, nerf_dart};
use pretty_assertions::assert_eq;

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

/// Upstream's
/// [`removePort`](https://github.com/pnpm/pnpm/blob/601317e7a3/network/auth-header/src/helpers/removePort.ts)
/// strips *any* port and retries iff the URL changed, not only
/// protocol defaults. A `.npmrc` keyed at host-only must still match
/// a request that explicitly carries a non-default port. A dev or
/// proxy registry on `:8080` matched by a `//host/` token is the
/// canonical case.
#[test]
fn non_default_port_strips_for_fallback_lookup() {
    let headers = build(&[("//reg.com/", "Bearer abc123")]);
    assert_eq!(headers.for_url("https://reg.com:8080/").as_deref(), Some("Bearer abc123"));
}

/// Upstream's [`@pnpm/config.nerf-dart`](https://github.com/pnpm/components/blob/a8ba7794d8/config/nerf-dart/nerf-dart.ts)
/// builds keys via WHATWG `URL.host`, which drops protocol-default
/// ports. A registry configured as `https://reg.com:443/` keys
/// creds at `//reg.com/` (not `//reg.com:443/`); a request to
/// `https://reg.com/` (no port) must match without the port-strip
/// fallback firing on the request side.
#[test]
fn nerf_dart_strips_default_ports_when_keying() {
    assert_eq!(nerf_dart("https://reg.com:443/"), "//reg.com/");
    assert_eq!(nerf_dart("http://reg.com:80/"), "//reg.com/");
    // Non-default ports are preserved.
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
    // Mirrors the GitHub Packages scope-registry example from
    // pnpm's test suite.
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

/// Upstream's
/// [`getAuthHeadersFromCreds`](https://github.com/pnpm/pnpm/blob/601317e7a3/network/auth-header/src/getAuthHeadersFromConfig.ts)
/// processes per-URI entries first, then unconditionally overwrites
/// the default-registry slot with the default-creds header. When a
/// `.npmrc` carries both `_authToken=A` (default) and
/// `//registry.npmjs.org/:_authToken=B` (per-URI for the default
/// registry), upstream guarantees the *default* (A) wins on the
/// default registry. Without the two-phase build in `from_creds_map`,
/// pacquet's `HashMap` iteration would let either value win
/// non-deterministically.
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
    // No URL → no match in any non-empty map.
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
