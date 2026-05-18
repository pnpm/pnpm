//! URL-keyed lookup of `Authorization` headers, ported from pnpm's
//! [`@pnpm/network.auth-header`](https://github.com/pnpm/pnpm/blob/601317e7a3/network/auth-header/src/index.ts).
//!
//! The lookup walks "nerf-darted" forms of a URL (the protocol-stripped
//! `//host[:port]/path/` representation npm has used for `.npmrc` keys
//! since the npm 5 era) from longest path prefix down to the host. If
//! the URL carries inline `user:password@`, that takes precedence and
//! is encoded as a `Basic` header even when no per-host token matches.
//!
//! The map is built once per install from the merged `.npmrc` and is
//! consulted on every metadata fetch and tarball download. The lookup
//! walks parts of the *request* URL: a tarball served from a CDN on a
//! different host than the registry only matches keys keyed at the
//! CDN's host (or a path prefix on that host). It does *not* fall
//! through to the registry's host. Pacquet matches pnpm here. If a
//! private registry redirects to its own subdomain or path, place a
//! key at that host or prefix in `.npmrc`; if it redirects across
//! hosts, no header is attached, matching upstream.

use std::collections::HashMap;

/// Bag of `Authorization` header values keyed by the nerf-darted form
/// of each registry URL. Pacquet builds one of these from the parsed
/// `.npmrc` and shares it across every HTTP call made during install.
///
/// Construct via [`AuthHeaders::from_creds_map`], [`AuthHeaders::from_map`],
/// or [`AuthHeaders::default`] (empty). Look up via [`AuthHeaders::for_url`].
#[derive(Debug, Default, Clone)]
pub struct AuthHeaders {
    /// Keys are the nerf-darted form (`//host[:port]/path/`). Values
    /// are ready-to-send header values like `Bearer abc123` or
    /// `Basic Zm9vOmJhcg==`.
    by_uri: HashMap<String, String>,
    /// The longest key in `by_uri` measured in `/`-separated parts. The
    /// lookup walks from this depth down to 3 (the `//host/` floor),
    /// matching pnpm's `getMaxParts` precomputation.
    max_parts: usize,
}

impl AuthHeaders {
    /// Build an [`AuthHeaders`] from `(nerf_darted_uri, header_value)`
    /// pairs. Caller is responsible for nerf-darting and for choosing
    /// the right scheme (`Bearer ...` or `Basic ...`).
    ///
    /// The `default_registry_url` argument is a full registry URL
    /// (e.g. `"https://registry.npmjs.org/"`, scheme included) that
    /// the constructor nerf-darts internally to derive the key for the
    /// empty-string ("default") credentials slot. Mirrors
    /// `createGetAuthHeaderByURI`'s `defaultRegistry` argument; falls
    /// back to `"//registry.npmjs.org/"` when `None`. Passing an
    /// already-nerf-darted `//host/.../` here would re-nerf-dart it to
    /// the empty string, silently masking default creds — pass the
    /// raw URL.
    pub fn from_creds_map<Iter>(headers: Iter, default_registry_url: Option<&str>) -> Self
    where
        Iter: IntoIterator<Item = (String, String)>,
    {
        let registry_default_key =
            default_registry_url.map(nerf_dart).unwrap_or_else(|| "//registry.npmjs.org/".into());
        let mut by_uri = HashMap::new();
        let mut default_header: Option<String> = None;
        // Two-phase build, mirroring upstream's
        // [`getAuthHeadersFromCreds`](https://github.com/pnpm/pnpm/blob/601317e7a3/network/auth-header/src/getAuthHeadersFromConfig.ts):
        // per-URI entries land first, then the default-registry creds
        // unconditionally overwrite the slot at `registry_default_key`.
        // Without the two-phase split, both entries would race through a
        // single HashMap insert and the winner would depend on
        // non-deterministic iteration order.
        for (raw_uri, header_value) in headers {
            if raw_uri.is_empty() {
                default_header = Some(header_value);
            } else {
                by_uri.insert(raw_uri, header_value);
            }
        }
        if let Some(header) = default_header {
            by_uri.insert(registry_default_key, header);
        }
        Self::from_map(by_uri)
    }

    /// Build an [`AuthHeaders`] directly from an already-keyed map.
    /// Each key must already be in nerf-darted form
    /// (`//host[:port]/path/`).
    pub fn from_map(by_uri: HashMap<String, String>) -> Self {
        let max_parts = by_uri.keys().map(|key| key.split('/').count()).max().unwrap_or(0);
        AuthHeaders { by_uri, max_parts }
    }

    /// Resolve an `Authorization` header for `url`, mirroring pnpm's
    /// `getAuthHeaderByURI`:
    ///
    /// 1. If `url` has a `user:password@` prefix, return `Basic` of it,
    ///    regardless of whether anything matched in the map.
    /// 2. Otherwise nerf-dart the URL and walk parent path prefixes
    ///    down to the host-only key.
    /// 3. If the URL carried any explicit port, retry the lookup with
    ///    the port stripped. Mirrors pnpm's
    ///    [`removePort`](https://github.com/pnpm/pnpm/blob/601317e7a3/network/auth-header/src/helpers/removePort.ts),
    ///    which strips *any* port (not just protocol defaults) and
    ///    retries iff the URL changed.
    pub fn for_url(&self, url: &str) -> Option<String> {
        // Append a trailing `/` first, matching pnpm's lookup which
        // does the same before parsing. Without this, a URL like
        // `https://npm.pkg.github.com/pnpm` (registry without
        // trailing slash) would nerf-dart to `//npm.pkg.github.com/`
        // and miss a `//npm.pkg.github.com/pnpm/` token.
        let mut owned: String;
        let url_with_slash = if url.ends_with('/') {
            url
        } else {
            owned = String::with_capacity(url.len() + 1);
            owned.push_str(url);
            owned.push('/');
            owned.as_str()
        };
        let parsed = ParsedUrl::parse(url_with_slash)?;
        if let Some(basic) = parsed.basic_auth_header() {
            return Some(basic);
        }
        if let Some(value) = self.lookup_by_nerf(&parsed) {
            return Some(value.to_owned());
        }
        if parsed.port.is_some() {
            let stripped = parsed.with_port_stripped();
            return self.lookup_by_nerf(&stripped).map(str::to_owned);
        }
        None
    }

    fn lookup_by_nerf(&self, parsed: &ParsedUrl<'_>) -> Option<&str> {
        if self.by_uri.is_empty() {
            return None;
        }
        let nerfed = parsed.nerf_dart();
        let parts: Vec<&str> = nerfed.split('/').collect();
        let upper = parts.len().min(self.max_parts);
        // Walk from the longest meaningful prefix down to `//host/`,
        // matching the index range `[maxParts-1, 3]` from
        // `getAuthHeaderByURI`. `parts[0..3]` is `["", "", host]`, so
        // joined with `/` it is `//host`; the loop slices through
        // `parts[..i]` and re-joins, then appends a trailing slash.
        // Exclusive upper bound mirrors upstream's `Math.min(parts.length,
        // maxParts) - 1`; the included extra iteration would always build
        // a key ending in `//` (the trailing empty segment from
        // `nerfed.split('/')` plus the appended `/`) and never match.
        for i in (3..upper).rev() {
            let key = format!("{}/", parts[..i].join("/"));
            if let Some(value) = self.by_uri.get(&key) {
                return Some(value.as_str());
            }
        }
        None
    }
}

/// Strip protocol, query string, fragment, basic-auth, and any
/// trailing characters past the path's final `/`, returning the
/// canonical "nerf-darted" form npm uses as `.npmrc` keys.
///
/// Examples:
/// * `https://reg.com/` → `//reg.com/`
/// * `https://reg.com:8080/` → `//reg.com:8080/`
/// * `https://reg.com/foo/-/foo-1.tgz` → `//reg.com/foo/-/`
/// * `https://user:pw@reg.com/scoped/pkg` → `//reg.com/scoped/`
/// * `https://npm.pkg.github.com/pnpm` (no trailing slash) → `//npm.pkg.github.com/`
pub fn nerf_dart(url: &str) -> String {
    let parsed = match ParsedUrl::parse(url) {
        Some(parsed) => parsed,
        None => return String::new(),
    };
    parsed.nerf_dart()
}

/// Lightweight URL parsing tuned for the subset of URLs `.npmrc` and
/// registries actually carry: `http`/`https` only, optional `user:pw@`,
/// optional `:port`, optional path. Standard library has no URL type
/// and pulling in the full `url` crate just for this is heavier than
/// needed.
#[derive(Clone, Copy)]
struct ParsedUrl<'a> {
    scheme: &'a str,
    user_info: Option<&'a str>,
    host: &'a str,
    port: Option<&'a str>,
    path: &'a str,
}

impl<'a> ParsedUrl<'a> {
    fn parse(url: &'a str) -> Option<Self> {
        let (scheme, rest) = url.split_once("://")?;
        // Strip query string and fragment. Neither participates in
        // nerf-darting per `removeFragment` / `removeSearch` in npm's
        // own implementation.
        let rest = rest.split(['?', '#']).next().unwrap_or(rest);
        let (authority, path) = match rest.split_once('/') {
            Some((authority, path_tail)) => (authority, path_tail),
            None => (rest, ""),
        };
        let (user_info, host_port) = match authority.rsplit_once('@') {
            Some((user_info, host_port)) => (Some(user_info), host_port),
            None => (None, authority),
        };
        let (host, port) = match host_port.rsplit_once(':') {
            // Skip IPv6 brackets. Pnpm doesn't handle them either, and
            // no npm registry we care about uses them. Documenting the
            // limit here rather than silently misparsing.
            Some((host, port)) if !host.contains('[') => (host, Some(port)),
            _ => (host_port, None),
        };
        Some(ParsedUrl { scheme, user_info, host, port, path })
    }

    fn nerf_dart(&self) -> String {
        let mut out = String::with_capacity(2 + self.host.len() + self.path.len());
        out.push_str("//");
        out.push_str(self.host);
        // Drop default ports the way upstream's WHATWG `URL.host` does
        // (`//reg.com:443/` → `//reg.com/`). Without this, a registry
        // configured as `https://reg.com:443/` keys creds at
        // `//reg.com:443/` and a request to `https://reg.com/...` (no
        // port) misses, because the port-strip fallback only fires
        // when the *request* URL carries a port. See
        // [`@pnpm/config.nerf-dart`](https://github.com/pnpm/components/blob/a8ba7794d8/config/nerf-dart/nerf-dart.ts).
        if let Some(port) = self.port
            && !is_default_port(self.scheme, port)
        {
            out.push(':');
            out.push_str(port);
        }
        out.push('/');
        // Drop everything after the last `/` in the path. That final
        // segment is a filename or package selector, not a key.
        let trimmed = match self.path.rfind('/') {
            Some(index) => &self.path[..index],
            None => "",
        };
        if !trimmed.is_empty() {
            out.push_str(trimmed);
            out.push('/');
        }
        out
    }

    fn basic_auth_header(&self) -> Option<String> {
        let user_info = self.user_info?;
        let (user, pass) = match user_info.split_once(':') {
            Some((user, pass)) => (user, pass),
            None => (user_info, ""),
        };
        if user.is_empty() && pass.is_empty() {
            return None;
        }
        Some(format!("Basic {}", base64_encode(&format!("{user}:{pass}"))))
    }

    fn with_port_stripped(&self) -> ParsedUrl<'a> {
        ParsedUrl { port: None, ..*self }
    }
}

fn is_default_port(scheme: &str, port: &str) -> bool {
    matches!((scheme, port), ("https", "443") | ("http", "80"))
}

/// Local base64 encode so this crate doesn't pull in `base64` just for
/// 4 lines. Standard alphabet, with padding, matching `btoa` /
/// `Buffer.from(...).toString('base64')` from the JS port.
pub fn base64_encode(input: &str) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let bytes = input.as_bytes();
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    let mut chunks = bytes.chunks_exact(3);
    for chunk in &mut chunks {
        let n = (u32::from(chunk[0]) << 16) | (u32::from(chunk[1]) << 8) | u32::from(chunk[2]);
        out.push(ALPHABET[((n >> 18) & 0x3f) as usize] as char);
        out.push(ALPHABET[((n >> 12) & 0x3f) as usize] as char);
        out.push(ALPHABET[((n >> 6) & 0x3f) as usize] as char);
        out.push(ALPHABET[(n & 0x3f) as usize] as char);
    }
    let remainder = chunks.remainder();
    match remainder.len() {
        1 => {
            let n = u32::from(remainder[0]) << 16;
            out.push(ALPHABET[((n >> 18) & 0x3f) as usize] as char);
            out.push(ALPHABET[((n >> 12) & 0x3f) as usize] as char);
            out.push('=');
            out.push('=');
        }
        2 => {
            let n = (u32::from(remainder[0]) << 16) | (u32::from(remainder[1]) << 8);
            out.push(ALPHABET[((n >> 18) & 0x3f) as usize] as char);
            out.push(ALPHABET[((n >> 12) & 0x3f) as usize] as char);
            out.push(ALPHABET[((n >> 6) & 0x3f) as usize] as char);
            out.push('=');
        }
        _ => {}
    }
    out
}

#[cfg(test)]
mod tests {
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
        let headers =
            build(&[("//reg.com/", "Bearer abc123"), ("//reg.co/tarballs/", "Bearer xxx")]);
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
        let headers =
            build(&[("//custom.domain.com/artifactory/api/npm/npm-virtual/", "Bearer xyz")]);
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
    /// pacquet's HashMap iteration would let either value win
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
}
