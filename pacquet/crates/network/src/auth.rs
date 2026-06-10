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
            default_registry_url.map_or_else(|| "//registry.npmjs.org/".into(), nerf_dart);
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
    #[must_use]
    pub fn from_map(by_uri: HashMap<String, String>) -> Self {
        let max_parts = by_uri.keys().map(|key| key.split('/').count()).max().unwrap_or(0);
        AuthHeaders { by_uri, max_parts }
    }

    /// The `(nerf_darted_uri, header_value)` pairs backing this lookup, so
    /// a caller can forward the whole set to another process (the pnpr
    /// resolver) and rebuild it with [`Self::from_map`].
    pub fn entries(&self) -> impl Iterator<Item = (&str, &str)> {
        self.by_uri.iter().map(|(uri, value)| (uri.as_str(), value.as_str()))
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
#[must_use]
pub fn nerf_dart(url: &str) -> String {
    let Some(parsed) = ParsedUrl::parse(url) else { return String::new() };
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
#[must_use]
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
mod tests;
