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

use std::{
    collections::{BTreeMap, HashMap},
    fmt,
    sync::Arc,
};

pub const DEFAULT_REGISTRY_SCOPE: &str = "@";

pub type AuthHeadersByScope = BTreeMap<String, BTreeMap<String, String>>;

/// Server-side override for upstream auth selection.
///
/// A plain [`AuthHeaders`] answers "what `Authorization` header does the
/// client's `.npmrc` attach to this URL?" — the right question for the
/// pnpm CLI, which fetches as the user. A server (pnpr) that resolves on
/// behalf of many callers must instead answer "what credential does *this
/// deployment's route policy* attach to this fetch, for this caller?" and
/// record which private route was touched so the result can be cached
/// without leaking one caller's private resolution to another.
///
/// When a hook is attached via [`AuthHeaders::with_route_hook`], every
/// [`AuthHeaders::for_url`] / [`AuthHeaders::for_url_with_package`] lookup
/// is delegated to it: the client-forwarded credentials carried by the
/// [`AuthHeaders`] are ignored, and the hook alone decides the header
/// (returning `None` for an anonymous/public fetch) and records the
/// route. A `None` hook (the CLI case) leaves lookup behavior unchanged.
pub trait UpstreamRouteHook: Send + Sync {
    /// Decide the `Authorization` header value for a fetch to `url` for
    /// package `package` (`None` for non-package fetches), and record the
    /// route the decision selected. `None` means fetch anonymously.
    fn authorize(&self, url: &str, package: Option<&str>) -> Option<String>;

    /// Classify the metadata cache scope for a fetch to `url` for package
    /// `package` (`None` for non-package fetches). Unlike [`Self::authorize`]
    /// this is a read-only query — it must **not** record into the resolve's
    /// footprint — so the resolver can pick the on-disk mirror namespace and
    /// in-memory/fetch-lock keys without double-counting a route.
    ///
    /// Defaults to [`MetadataCacheScope::Public`] for hooks that don't
    /// partition metadata by route.
    fn metadata_scope(&self, _url: &str, _package: Option<&str>) -> MetadataCacheScope {
        MetadataCacheScope::Public
    }
}

/// The cache namespace a metadata fetch for one `(registry, package)` route
/// belongs to, decided once per fetch from the route policy. A server (pnpr)
/// that resolves on behalf of many callers must keep one caller's private
/// metadata out of the global mirror every other caller reads; this enum is
/// how the route decision reaches the npm resolver's mirror path, in-memory
/// cache key, and fetch-lock key.
///
/// The pnpm CLI has no route hook, so every fetch is [`Self::Public`] and the
/// global mirror behaves exactly as before.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MetadataCacheScope {
    /// Public route: the shared, global metadata mirror — current behavior,
    /// shared by every caller.
    Public,
    /// Private route keyed by a private access descriptor. `descriptor_id`
    /// is a filesystem-safe, server-secret-keyed digest that namespaces the
    /// on-disk mirror, in-memory cache, and fetch lock, so one caller's
    /// private metadata never satisfies a fetch for a caller who does not
    /// reproduce the same descriptor.
    Private { descriptor_id: String },
}

/// Bag of `Authorization` header values keyed by the nerf-darted form
/// of each registry URL. Pacquet builds one of these from the parsed
/// `.npmrc` and shares it across every HTTP call made during install.
///
/// Construct via [`AuthHeaders::from_parts`], [`AuthHeaders::from_creds_map`],
/// [`AuthHeaders::from_map`], or [`AuthHeaders::default`] (empty). Look up via
/// [`AuthHeaders::for_url`].
#[derive(Default, Clone)]
pub struct AuthHeaders {
    /// Keys are the nerf-darted form (`//host[:port]/path/`). Values
    /// are ready-to-send header values like `Bearer abc123` or
    /// `Basic Zm9vOmJhcg==`.
    by_uri: HashMap<String, String>,
    /// Package-scope credentials keyed as
    /// `scoped_by_scope[scope][registry_uri]`, where `registry_uri` is
    /// the nerf-darted registry URL without the trailing scope segment.
    scoped_by_scope: HashMap<String, HashMap<String, String>>,
    /// The longest key in `by_uri` measured in `/`-separated parts. The
    /// lookup walks from this depth down to 3 (the `//host/` floor),
    /// matching pnpm's `getMaxParts` precomputation.
    max_parts: usize,
    /// The longest registry key per package scope, measured the same
    /// way as `max_parts`.
    max_scoped_parts_by_scope: HashMap<String, usize>,
    /// Server-side route hook. When set, it owns every auth lookup and
    /// the client-forwarded credentials above are ignored. See
    /// [`UpstreamRouteHook`].
    route_hook: Option<Arc<dyn UpstreamRouteHook>>,
}

impl fmt::Debug for AuthHeaders {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Header values carry credentials, so the maps' *contents* must
        // never reach a log line; show only key counts plus whether a
        // server route hook is overriding lookup.
        f.debug_struct("AuthHeaders")
            .field("by_uri", &self.by_uri.len())
            .field("scoped_by_scope", &self.scoped_by_scope.len())
            .field("route_hook", &self.route_hook.is_some())
            .finish_non_exhaustive()
    }
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
                by_uri.insert(normalize_auth_key(raw_uri), header_value);
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
    pub fn from_map(headers: HashMap<String, String>) -> Self {
        let mut by_uri = HashMap::new();
        let mut scoped_by_uri: HashMap<String, HashMap<String, String>> = HashMap::new();
        for (uri, value) in headers {
            let uri = normalize_auth_key(uri);
            if let Some((registry_uri, scope)) = split_scoped_auth_key(&uri) {
                scoped_by_uri.entry(registry_uri).or_default().insert(scope, value);
            } else {
                by_uri.insert(uri, value);
            }
        }
        Self::from_parts(by_uri, scoped_by_uri)
    }

    /// Build an [`AuthHeaders`] from already-structured registry and
    /// package-scope header maps.
    #[must_use]
    pub fn from_parts(
        by_uri: HashMap<String, String>,
        scoped_by_uri: HashMap<String, HashMap<String, String>>,
    ) -> Self {
        let by_uri: HashMap<String, String> =
            by_uri.into_iter().map(|(uri, value)| (normalize_auth_key(uri), value)).collect();
        let mut scoped_by_scope: HashMap<String, HashMap<String, String>> = HashMap::new();
        let mut max_scoped_parts_by_scope: HashMap<String, usize> = HashMap::new();
        for (uri, scoped) in scoped_by_uri {
            let uri = normalize_auth_key(uri);
            let parts = uri.split('/').count();
            for (scope, value) in scoped {
                max_scoped_parts_by_scope
                    .entry(scope.clone())
                    .and_modify(|max| *max = (*max).max(parts))
                    .or_insert(parts);
                scoped_by_scope.entry(scope).or_default().insert(uri.clone(), value);
            }
        }
        let max_parts = by_uri.keys().map(|key| key.split('/').count()).max().unwrap_or(0);
        AuthHeaders {
            by_uri,
            scoped_by_scope,
            max_parts,
            max_scoped_parts_by_scope,
            route_hook: None,
        }
    }

    /// Build an [`AuthHeaders`] from the structured pnpr wire shape:
    /// `auth_headers[registry_uri][scope]`. The `@` scope stores
    /// registry-wide auth.
    #[must_use]
    pub fn from_by_scope(headers: AuthHeadersByScope) -> Self {
        let mut by_uri = HashMap::new();
        let mut scoped_by_uri: HashMap<String, HashMap<String, String>> = HashMap::new();
        for (uri, headers_by_scope) in headers {
            let uri = normalize_auth_key(uri);
            for (scope, value) in headers_by_scope {
                if scope == DEFAULT_REGISTRY_SCOPE {
                    by_uri.insert(uri.clone(), value);
                } else {
                    scoped_by_uri.entry(uri.clone()).or_default().insert(scope, value);
                }
            }
        }
        Self::from_parts(by_uri, scoped_by_uri)
    }

    /// The structured `auth_headers[registry_uri][scope]` map backing
    /// this lookup, suitable for forwarding to a pnpr resolver.
    #[must_use]
    pub fn to_by_scope(&self) -> AuthHeadersByScope {
        let mut result = AuthHeadersByScope::new();
        for (uri, value) in &self.by_uri {
            result
                .entry(uri.clone())
                .or_default()
                .insert(DEFAULT_REGISTRY_SCOPE.to_owned(), value.clone());
        }
        for (scope, scoped_by_uri) in &self.scoped_by_scope {
            for (registry_uri, value) in scoped_by_uri {
                result
                    .entry(registry_uri.clone())
                    .or_default()
                    .insert(scope.clone(), value.clone());
            }
        }
        result
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
    #[must_use]
    pub fn for_url(&self, url: &str) -> Option<String> {
        self.for_url_with_package(url, None)
    }

    /// Attach a server-side [`UpstreamRouteHook`] that takes over auth
    /// selection. The returned [`AuthHeaders`] keeps its
    /// client-forwarded credentials (so [`Self::to_by_scope`] still
    /// reflects them) but no longer consults them on lookup — the hook
    /// decides. Used by pnpr to resolve as the deployment's route policy
    /// rather than as the calling client.
    #[must_use]
    pub fn with_route_hook(mut self, hook: Arc<dyn UpstreamRouteHook>) -> Self {
        self.route_hook = Some(hook);
        self
    }

    /// Record the route for a metadata/tarball fetch that is about to be
    /// served from an in-memory or on-disk cache *without* an HTTP
    /// request, so a server [`UpstreamRouteHook`]'s footprint still
    /// reflects every private route the resolve depended on. The route is
    /// classified exactly as the real fetch would have (same `url`, same
    /// `pkg_name`); the credential the hook selects is discarded because
    /// no request is sent.
    ///
    /// No-op when no route hook is installed (the CLI case): a fetch that
    /// never happens needs no `Authorization` header, and the CLI keeps no
    /// footprint. Idempotent for the hook — recording the same route more
    /// than once collapses to one footprint entry.
    pub fn record_route(&self, url: &str, pkg_name: Option<&str>) {
        if let Some(hook) = &self.route_hook {
            hook.authorize(url, pkg_name);
        }
    }

    /// The metadata cache scope a fetch to `url` for `pkg_name` belongs to.
    /// A server route hook owns the decision; without one (the CLI case)
    /// every fetch is [`MetadataCacheScope::Public`], leaving the global
    /// mirror unchanged. Read-only — never records into a footprint.
    #[must_use]
    pub fn metadata_scope(&self, url: &str, pkg_name: Option<&str>) -> MetadataCacheScope {
        match &self.route_hook {
            Some(hook) => hook.metadata_scope(url, pkg_name),
            None => MetadataCacheScope::Public,
        }
    }

    /// Resolve an `Authorization` header for `url`, preferring
    /// package-scope credentials when `pkg_name` is scoped.
    #[must_use]
    pub fn for_url_with_package(&self, url: &str, pkg_name: Option<&str>) -> Option<String> {
        // A server route hook owns the decision: ignore the
        // client-forwarded credentials entirely (including any inline
        // `user:pass@` in `url`) and let the deployment's policy pick the
        // credential and record the route.
        if let Some(hook) = &self.route_hook {
            return hook.authorize(url, pkg_name);
        }
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
        if let Some(scope) = package_scope(pkg_name) {
            if let Some(value) = self.lookup_scope_by_nerf(&parsed, scope) {
                return Some(value.to_owned());
            }
            if parsed.port.is_some() {
                let stripped = parsed.with_port_stripped();
                if let Some(value) = self.lookup_scope_by_nerf(&stripped, scope) {
                    return Some(value.to_owned());
                }
            }
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

    fn lookup_scope_by_nerf(&self, parsed: &ParsedUrl<'_>, scope: &str) -> Option<&str> {
        let scoped_by_uri = self.scoped_by_scope.get(scope)?;
        let max_scoped_parts = self.max_scoped_parts_by_scope.get(scope).copied()?;
        let nerfed = parsed.nerf_dart();
        let parts: Vec<&str> = nerfed.split('/').collect();
        let upper = parts.len().min(max_scoped_parts);
        for i in (3..upper).rev() {
            let key = format!("{}/", parts[..i].join("/"));
            if let Some(value) = scoped_by_uri.get(&key) {
                return Some(value.as_str());
            }
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

fn normalize_auth_key(mut uri: String) -> String {
    if !uri.is_empty() && !uri.ends_with('/') {
        uri.push('/');
    }
    uri
}

fn split_scoped_auth_key(uri: &str) -> Option<(String, String)> {
    let trimmed = uri.strip_suffix('/').unwrap_or(uri);
    if let Some(scope_separator_index) = trimmed.rfind(":@") {
        let scope = &trimmed[scope_separator_index + 1..];
        if is_package_scope(scope) {
            return Some((
                normalize_auth_key(trimmed[..scope_separator_index].to_owned()),
                scope.to_owned(),
            ));
        }
    }
    let last_slash_index = trimmed.rfind('/')?;
    let scope = &trimmed[last_slash_index + 1..];
    if !is_package_scope(scope) {
        return None;
    }
    Some((trimmed[..=last_slash_index].to_owned(), scope.to_owned()))
}

fn is_package_scope(scope: &str) -> bool {
    scope.starts_with('@') && scope.len() > 1 && !scope.contains('/') && !scope.contains(':')
}

fn package_scope(pkg_name: Option<&str>) -> Option<&str> {
    let pkg_name = pkg_name?;
    if !pkg_name.starts_with('@') {
        return None;
    }
    let (scope, name) = pkg_name.split_once('/')?;
    if scope.len() <= 1 || name.is_empty() {
        return None;
    }
    Some(scope)
}

/// Strip protocol, query string, fragment, basic-auth, and any
/// trailing characters past the path's final `/`, returning the
/// canonical "nerf-darted" form npm uses as `.npmrc` keys.
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

/// Strip `user:pass@` (or `user@`) that appears right after a URL scheme in
/// any message text, e.g. `… https://user:pass@host/pkg …` →
/// `… https://host/pkg …`. A registry configured as `https://user:pass@host/`
/// would otherwise leak its embedded basic-auth into a fetch error or a retry
/// log line. Ports pnpm's
/// [`redactUrlCredentials`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/core/error/src/index.ts#L78-L101).
#[must_use]
pub fn redact_url_credentials(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(pos) = rest.find("://") {
        let (before, after) = rest.split_at(pos + "://".len());
        out.push_str(before);
        // Only treat "://" as a URL authority boundary when a scheme character
        // (schemes end in an ASCII alphanumeric) precedes it, so an unrelated
        // "://" in the message isn't mangled.
        let has_scheme = pos > 0 && rest.as_bytes()[pos - 1].is_ascii_alphanumeric();
        rest = strip_leading_userinfo(after).filter(|_| has_scheme).unwrap_or(after);
    }
    out.push_str(rest);
    out
}

/// Make untrusted, URL-bearing text safe to print or log: redact inline
/// `user:pass@` credentials ([`redact_url_credentials`]) and strip every
/// control character. Used for registry URLs and network-error messages
/// alike — both can carry basic-auth or escape sequences from an untrusted
/// `.npmrc` / `--registry` (or a `reqwest` error that echoes the request URL
/// back), which must not leak credentials or inject terminal output via raw
/// escapes / `\r` / `\n`.
#[must_use]
pub fn redact_and_sanitize(text: &str) -> String {
    redact_url_credentials(text).chars().filter(|character| !character.is_control()).collect()
}

/// If the authority leading `text` contains `userinfo@`, return the slice after
/// the **last** `@` within it; otherwise `None`. The authority ends at the first
/// `/`, `?`, `#`, or whitespace. Stripping to the last `@` keeps a raw `@` inside
/// the password (`user:p@ss@host`) from leaking its tail.
fn strip_leading_userinfo(authority: &str) -> Option<&str> {
    let mut last_at = None;
    for (idx, ch) in authority.char_indices() {
        match ch {
            '@' => last_at = Some(idx + ch.len_utf8()),
            '/' | '?' | '#' => break,
            c if c.is_whitespace() => break,
            _ => {}
        }
    }
    last_at.map(|end| &authority[end..])
}

#[cfg(test)]
mod tests;
