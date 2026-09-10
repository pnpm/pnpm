//! URL-keyed lookup of `Authorization` headers.
//!
//! The lookup walks "nerf-darted" forms of a URL (the protocol-stripped
//! `//host[:port]/path/` representation npm has used for `.npmrc` keys
//! since the npm 5 era) from longest path prefix down to the host. If
//! the URL carries inline `user:password@`, that takes precedence and
//! is encoded as a `Basic` header even when no per-host token matches.
//!
//! Configuration readers build the map once per install from their native
//! credential sources, and request code consults it on every metadata fetch
//! and archive download. The lookup
//! walks parts of the *request* URL: a tarball served from a CDN on a
//! different host than the registry only matches keys keyed at the
//! CDN's host (or a path prefix on that host). It does *not* fall
//! through to the registry's host. If a private registry redirects to
//! its own subdomain or path, place a key at that host or prefix in
//! `.npmrc`; if it redirects across hosts, no header is attached.

pub use redaction::{
    hide_auth_information, redact_and_sanitize, redact_and_sanitize_multiline,
    redact_url_credentials, redact_url_for_display,
};
pub use url::{base64_encode, base64_encode_bytes, is_url_secure_for_credentials, nerf_dart};

use crate::token_helper::{TokenHelperRunner, execute_token_helper, run_token_helper_command};
use std::{
    collections::{BTreeMap, HashMap},
    fmt,
    sync::{Arc, Mutex, OnceLock},
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

    /// Whether this deployment may fetch `url` at all.
    ///
    /// A server resolves on behalf of callers who describe their own
    /// registries, so a URL it was told about is not automatically one it may
    /// reach. Answered at the fetch itself rather than when the request is
    /// read, so a registry a caller merely configures — a scope it never
    /// resolves a package from — costs nothing, while one it does resolve
    /// from is refused before the request leaves the process.
    ///
    /// Defaults to `true` for hooks with no such policy (the CLI fetches as
    /// the user, who may reach whatever they configured).
    fn allows_fetch(&self, _url: &str) -> bool {
        true
    }

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
/// of each registry URL. Ecosystem-specific configuration readers normalize
/// their credentials into this request-facing form and share it across HTTP
/// calls made during install.
///
/// Construct via [`AuthHeaders::from_parts`], [`AuthHeaders::from_creds_map`],
/// [`AuthHeaders::from_map`], or [`AuthHeaders::default`] (empty). Look up via
/// [`AuthHeaders::for_url`].
/// Memo of resolved `tokenHelper` results keyed by `scope + map key`. Each
/// entry is a per-key [`OnceLock`] so a resolving subprocess runs without the
/// shared [`Mutex`] held. See the `resolved_token_helpers` field on
/// [`AuthHeaders`].
type TokenHelperCache = Arc<Mutex<HashMap<String, Arc<OnceLock<Option<String>>>>>>;

#[derive(Default, Clone)]
pub struct AuthHeaders {
    /// Keys are the nerf-darted form (`//host[:port]/path/`). Each value
    /// is either a ready-to-send header (`Bearer abc123`,
    /// `Basic Zm9vOmJhcg==`) or an un-executed `tokenHelper` command,
    /// resolved lazily on lookup (see [`AuthEntry`]).
    by_uri: HashMap<String, AuthEntry>,
    /// Package-scope credentials keyed as
    /// `scoped_by_scope[scope][registry_uri]`, where `registry_uri` is
    /// the nerf-darted registry URL without the trailing scope segment.
    scoped_by_scope: HashMap<String, HashMap<String, AuthEntry>>,
    /// The longest key in `by_uri` measured in `/`-separated parts. The
    /// lookup walks from this depth down to 3 (the `//host/` floor).
    max_parts: usize,
    /// The longest registry key per package scope, measured the same
    /// way as `max_parts`.
    max_scoped_parts_by_scope: HashMap<String, usize>,
    /// Server-side route hook. When set, it owns every auth lookup and
    /// the client-forwarded credentials above are ignored. See
    /// [`UpstreamRouteHook`].
    route_hook: Option<Arc<dyn UpstreamRouteHook>>,
    require_secure_transport: bool,
    /// Set iff any entry is an [`AuthEntry::TokenHelper`]. Surfaced in the
    /// [`fmt::Debug`] output (never the values) so a resolve trace shows
    /// at a glance whether any helper is configured. The lookup hot path
    /// touches the resolution cache only on a `TokenHelper` match, so a
    /// map of only baked headers pays nothing regardless.
    has_token_helpers: bool,
    /// Memoizes each resolved `tokenHelper`: a helper runs at most once
    /// per process, keyed by its map key. Each key maps to a per-key
    /// [`OnceLock`] so the resolving subprocess runs without the shared
    /// [`Mutex`] held — one slow helper can't block another registry's
    /// lookup. `Some(header)` on success, `None` on failure (so a failed
    /// helper is never retried and never falls back to sending a different
    /// credential). Shared across [`Clone`]s so the memo survives an `Arc`
    /// unwrap-and-rebuild.
    resolved_token_helpers: TokenHelperCache,
    /// Runner used to execute a `tokenHelper`. `None` uses the real
    /// process spawner; tests inject a fake via
    /// [`AuthHeaders::with_token_helper_runner`].
    token_helper_runner: Option<TokenHelperRunner>,
}

/// One resolved credential slot: either a baked header value or a
/// `tokenHelper` command still to be executed. Keeping both in the same
/// map preserves pnpm's single longest-path-prefix lookup — a
/// `tokenHelper` at `//host/` and a static token at `//host/path/`
/// compete by prefix length exactly as two static tokens would.
#[derive(Clone)]
enum AuthEntry {
    /// A ready-to-send `Authorization` header value.
    Header(String),
    /// A `[program, ...args]` command, executed lazily to a `Bearer …`
    /// (or scheme-carrying) header on first matching lookup.
    TokenHelper(Vec<String>),
}

impl fmt::Debug for AuthHeaders {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Header values carry credentials, so the maps' *contents* must
        // never reach a log line; show only key counts plus whether a
        // server route hook is overriding lookup.
        f.debug_struct("AuthHeaders")
            .field("by_uri", &self.by_uri.len())
            .field("scoped_by_scope", &self.scoped_by_scope.len())
            .field("has_token_helpers", &self.has_token_helpers)
            .field("route_hook", &self.route_hook.is_some())
            .field("require_secure_transport", &self.require_secure_transport)
            .finish_non_exhaustive()
    }
}

impl AuthHeaders {
    /// Restrict every credential lookup to TLS or loopback URLs, including
    /// lookups made by shared fetchers. This restriction survives cloning.
    #[must_use]
    pub fn with_secure_transport(mut self) -> Self {
        self.require_secure_transport = true;
        self
    }

    /// Whether no configured credential or route hook can provide
    /// authorization for any URL.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_uri.is_empty() && self.scoped_by_scope.is_empty() && self.route_hook.is_none()
    }

    /// Overlay a ready-to-send `Authorization` header at `url`.
    ///
    /// This is the boundary for credential sources that are already scoped to
    /// a concrete request URL and have no npm package-scope semantics. The
    /// caller owns the authentication scheme: npm tokens include `Bearer`,
    /// Cargo tokens are bare, and future readers may supply `Basic` or another
    /// registry-defined value. An invalid or unsupported URL is ignored.
    pub fn insert_url_header(&mut self, url: &str, header: String) {
        let mut terminated;
        let url = if url.ends_with('/') {
            url
        } else {
            terminated = String::with_capacity(url.len() + 1);
            terminated.push_str(url);
            terminated.push('/');
            &terminated
        };
        let uri = nerf_dart(url);
        if uri.is_empty() {
            return;
        }
        self.max_parts = self.max_parts.max(uri.split('/').count());
        self.by_uri.insert(uri, AuthEntry::Header(header));
    }

    /// Build an [`AuthHeaders`] from `(nerf_darted_uri, header_value)`
    /// pairs. Caller is responsible for nerf-darting and for choosing
    /// the right scheme (`Bearer ...` or `Basic ...`).
    ///
    /// There is no "default registry" slot: a credential is only ever
    /// honored at the URI it is keyed under. Callers pin an unscoped
    /// credential to the registry its own source declared before it gets
    /// here (see `NpmrcAuth::rescope_unscoped`), so the resolved default
    /// registry — which repository-controlled config can move — never
    /// decides where a credential is sent. An entry with an empty URI is
    /// dropped.
    pub fn from_creds_map<Iter>(headers: Iter) -> Self
    where
        Iter: IntoIterator<Item = (String, String)>,
    {
        Self::from_map(
            headers
                .into_iter()
                .filter(|(uri, _)| !uri.is_empty())
                // Normalize before collecting: two spellings of one URI
                // (`//reg.com` and `//reg.com/`) must collapse here rather
                // than survive as distinct keys and race in [`Self::from_map`].
                .map(|(uri, header)| (normalize_auth_key(uri), header))
                .collect(),
        )
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
        Self::from_parts_with_token_helpers(by_uri, scoped_by_uri, HashMap::new(), HashMap::new())
    }

    /// Build an [`AuthHeaders`] from baked header maps plus un-executed
    /// `tokenHelper` command maps. A `tokenHelper` at a given key wins
    /// over a baked header at the same key (pnpm resolves `tokenHelper`
    /// before a static token). The helper commands are run lazily on
    /// lookup.
    #[must_use]
    pub fn from_parts_with_token_helpers(
        by_uri: HashMap<String, String>,
        scoped_by_uri: HashMap<String, HashMap<String, String>>,
        token_helper_by_uri: HashMap<String, Vec<String>>,
        token_helper_scoped_by_uri: HashMap<String, HashMap<String, Vec<String>>>,
    ) -> Self {
        let mut by_uri_entries: HashMap<String, AuthEntry> = by_uri
            .into_iter()
            .map(|(uri, value)| (normalize_auth_key(uri), AuthEntry::Header(value)))
            .collect();
        for (uri, command) in token_helper_by_uri {
            by_uri_entries.insert(normalize_auth_key(uri), AuthEntry::TokenHelper(command));
        }

        let mut scoped_entries: HashMap<String, HashMap<String, AuthEntry>> = HashMap::new();
        for (uri, scoped) in scoped_by_uri {
            let uri = normalize_auth_key(uri);
            let entry = scoped_entries.entry(uri).or_default();
            for (scope, value) in scoped {
                entry.insert(scope, AuthEntry::Header(value));
            }
        }
        for (uri, scoped) in token_helper_scoped_by_uri {
            let uri = normalize_auth_key(uri);
            let entry = scoped_entries.entry(uri).or_default();
            for (scope, command) in scoped {
                entry.insert(scope, AuthEntry::TokenHelper(command));
            }
        }

        Self::from_entry_parts(by_uri_entries, scoped_entries)
    }

    /// Assemble the lookup indices from already-wrapped [`AuthEntry`]
    /// maps: the per-scope longest-key counts, the top-level
    /// `max_parts`, and the `has_token_helpers` gate.
    fn from_entry_parts(
        by_uri: HashMap<String, AuthEntry>,
        scoped_by_uri: HashMap<String, HashMap<String, AuthEntry>>,
    ) -> Self {
        let mut scoped_by_scope: HashMap<String, HashMap<String, AuthEntry>> = HashMap::new();
        let mut max_scoped_parts_by_scope: HashMap<String, usize> = HashMap::new();
        let mut has_token_helpers =
            by_uri.values().any(|entry| matches!(entry, AuthEntry::TokenHelper(_)));
        for (uri, scoped) in scoped_by_uri {
            let parts = uri.split('/').count();
            for (scope, value) in scoped {
                has_token_helpers |= matches!(value, AuthEntry::TokenHelper(_));
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
            require_secure_transport: false,
            has_token_helpers,
            resolved_token_helpers: Arc::default(),
            token_helper_runner: None,
        }
    }

    /// Override the runner used to execute `tokenHelper` commands. The
    /// dependency-injection seam for tests: production leaves it unset
    /// and spawns real processes.
    #[must_use]
    pub fn with_token_helper_runner(mut self, runner: TokenHelperRunner) -> Self {
        self.token_helper_runner = Some(runner);
        self
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
        // A `tokenHelper` entry has no static string to forward — it would
        // have to be executed — so it is skipped here. This map feeds the
        // pnpr wire shape, which carries only resolved header strings.
        let mut result = AuthHeadersByScope::new();
        for (uri, entry) in &self.by_uri {
            if let AuthEntry::Header(value) = entry {
                result
                    .entry(uri.clone())
                    .or_default()
                    .insert(DEFAULT_REGISTRY_SCOPE.to_owned(), value.clone());
            }
        }
        for (scope, scoped_by_uri) in &self.scoped_by_scope {
            for (registry_uri, entry) in scoped_by_uri {
                if let AuthEntry::Header(value) = entry {
                    result
                        .entry(registry_uri.clone())
                        .or_default()
                        .insert(scope.clone(), value.clone());
                }
            }
        }
        result
    }

    /// Resolve an `Authorization` header for `url`:
    ///
    /// 1. If `url` has a `user:password@` prefix, return `Basic` of it,
    ///    regardless of whether anything matched in the map.
    /// 2. Otherwise nerf-dart the URL and walk parent path prefixes
    ///    down to the host-only key.
    /// 3. If the URL carried any explicit port, retry the lookup with
    ///    the port stripped — stripping *any* port (not just protocol
    ///    defaults) and retrying iff the URL changed.
    #[must_use]
    pub fn for_url(&self, url: &str) -> Option<String> {
        self.for_url_with_package(url, None)
    }

    /// Resolve an `Authorization` header only when `url` uses TLS or targets
    /// the local machine. The loopback exception keeps local registry proxies
    /// usable without exposing credentials on a network link.
    #[must_use]
    pub fn for_secure_url(&self, url: &str) -> Option<String> {
        self.for_secure_url_with_package(url, None)
    }

    /// Package-aware counterpart to [`Self::for_secure_url`].
    #[must_use]
    pub fn for_secure_url_with_package(&self, url: &str, pkg_name: Option<&str>) -> Option<String> {
        if !is_url_secure_for_credentials(url) {
            return None;
        }
        self.for_url_with_package(url, pkg_name)
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

    /// Whether the fetch to `url` is permitted, per an attached
    /// [`UpstreamRouteHook`]. Always true without one — see
    /// [`UpstreamRouteHook::allows_fetch`].
    #[must_use]
    pub fn allows_fetch(&self, url: &str) -> bool {
        self.route_hook.as_ref().is_none_or(|hook| hook.allows_fetch(url))
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
}

/// Canonicalize an auth-map key to the trailing-slash form the lookup
/// compares against, so `//reg.com` and `//reg.com/` cannot coexist as
/// two entries for one registry.
#[must_use]
pub fn normalize_auth_key(mut uri: String) -> String {
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

#[cfg(test)]
mod tests;

mod redaction;

mod url;
use url::ParsedUrl;

mod lookup;
