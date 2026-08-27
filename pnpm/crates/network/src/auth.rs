//! URL-keyed lookup of `Authorization` headers.
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
//! through to the registry's host. If a private registry redirects to
//! its own subdomain or path, place a key at that host or prefix in
//! `.npmrc`; if it redirects across hosts, no header is attached.

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
/// of each registry URL. Pacquet builds one of these from the parsed
/// `.npmrc` and shares it across every HTTP call made during install.
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
            .finish_non_exhaustive()
    }
}

impl AuthHeaders {
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
        // Append a trailing `/` before parsing. Without this, a URL like
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
        // Each lookup returns `None` when no key matched (so the walk
        // falls through to the next candidate) and `Some(_)` when a key
        // matched — even `Some(None)`, a matched `tokenHelper` that failed
        // to resolve. A match is final: pnpm's most-specific key owns the
        // decision, so a failed helper must not fall back to a shorter
        // prefix or a different scope and send another credential.
        if let Some(scope) = package_scope(pkg_name) {
            if let Some(resolved) = self.lookup_scope_by_nerf(&parsed, scope) {
                return resolved;
            }
            if parsed.port.is_some() {
                let stripped = parsed.with_port_stripped();
                if let Some(resolved) = self.lookup_scope_by_nerf(&stripped, scope) {
                    return resolved;
                }
            }
        }
        if let Some(resolved) = self.lookup_by_nerf(&parsed) {
            return resolved;
        }
        if parsed.port.is_some() {
            let stripped = parsed.with_port_stripped();
            if let Some(resolved) = self.lookup_by_nerf(&stripped) {
                return resolved;
            }
        }
        None
    }

    /// Walk package-scope keys for `scope` longest-prefix first. Returns
    /// `None` when nothing matched and `Some(resolved)` when a key
    /// matched (the inner `Option` is the resolved header, `None` if a
    /// matched `tokenHelper` failed).
    fn lookup_scope_by_nerf(&self, parsed: &ParsedUrl<'_>, scope: &str) -> Option<Option<String>> {
        let scoped_by_uri = self.scoped_by_scope.get(scope)?;
        let max_scoped_parts = self.max_scoped_parts_by_scope.get(scope).copied()?;
        let nerfed = parsed.nerf_dart();
        let parts: Vec<&str> = nerfed.split('/').collect();
        let upper = parts.len().min(max_scoped_parts);
        for i in (3..upper).rev() {
            let key = format!("{}/", parts[..i].join("/"));
            if let Some(entry) = scoped_by_uri.get(&key) {
                return Some(self.resolve_entry(&key, scope, entry));
            }
        }
        None
    }

    fn lookup_by_nerf(&self, parsed: &ParsedUrl<'_>) -> Option<Option<String>> {
        if self.by_uri.is_empty() {
            return None;
        }
        let nerfed = parsed.nerf_dart();
        let parts: Vec<&str> = nerfed.split('/').collect();
        let upper = parts.len().min(self.max_parts);
        // Walk from the longest meaningful prefix down to `//host/`.
        // `parts[0..3]` is `["", "", host]`, so joined with `/` it is
        // `//host`; the loop slices through `parts[..i]` and re-joins,
        // then appends a trailing slash. The exclusive upper bound at
        // `min(parts.len(), max_parts)` drops the extra iteration that
        // would always build a key ending in `//` (the trailing empty
        // segment from `nerfed.split('/')` plus the appended `/`) and
        // never match.
        for i in (3..upper).rev() {
            let key = format!("{}/", parts[..i].join("/"));
            if let Some(entry) = self.by_uri.get(&key) {
                return Some(self.resolve_entry(&key, DEFAULT_REGISTRY_SCOPE, entry));
            }
        }
        None
    }

    /// Resolve a matched [`AuthEntry`] to a header value. A baked header
    /// is cloned; a `tokenHelper` is executed once and memoized (keyed by
    /// `scope` + map key so the same command at two registries still runs
    /// per registry). A helper failure logs and yields `None` — pacquet
    /// never sends a wrong, partial, or stale credential.
    fn resolve_entry(&self, key: &str, scope: &str, entry: &AuthEntry) -> Option<String> {
        match entry {
            AuthEntry::Header(value) => Some(value.clone()),
            AuthEntry::TokenHelper(command) => {
                let cache_key = format!("{scope}\u{0}{key}");
                // Take the per-key cell out under the global lock, then
                // release it *before* running the helper. Holding the lock
                // across the (up to `TOKEN_HELPER_TIMEOUT`) subprocess would
                // block every other registry's lookup on one slow helper.
                // `OnceLock` still serializes concurrent first-lookups of the
                // *same* key, so the command runs at most once.
                let cell = {
                    let mut cache = self
                        .resolved_token_helpers
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    Arc::clone(cache.entry(cache_key).or_default())
                };
                cell.get_or_init(|| {
                    let runner = self.token_helper_runner.unwrap_or(run_token_helper_command);
                    match execute_token_helper(command, runner) {
                        Ok(header) => Some(header),
                        Err(error) => {
                            let program = command.first().map_or("", String::as_str);
                            tracing::error!(
                                target: "pacquet::auth",
                                "token helper {program:?} failed; the request will be sent \
                                 without authentication: {error}",
                            );
                            None
                        }
                    }
                })
                .clone()
            }
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
        // Drop default ports (`//reg.com:443/` → `//reg.com/`). Without
        // this, a registry configured as `https://reg.com:443/` keys
        // creds at `//reg.com:443/` and a request to
        // `https://reg.com/...` (no port) misses, because the port-strip
        // fallback only fires when the *request* URL carries a port.
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
/// 4 lines. Standard alphabet, with padding.
#[must_use]
pub fn base64_encode(input: &str) -> String {
    base64_encode_bytes(input.as_bytes())
}

/// [`base64_encode`] for credentials that are not required to be UTF-8,
/// so a re-encoded `.npmrc` credential reproduces its bytes exactly.
#[must_use]
pub fn base64_encode_bytes(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
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

/// Mask an `Authorization` header value for display in an error message.
/// The scheme survives so the reader can tell a `Bearer` token from `Basic`
/// credentials, and a token long enough to be recognized by its owner keeps
/// its first four characters; everything else becomes `[hidden]`.
///
/// Control characters are dropped first: the value comes from an untrusted
/// `.npmrc` / environment variable and the masked result is printed to the
/// terminal, so a token carrying raw escapes could otherwise inject terminal
/// output through the characters masking leaves behind.
#[must_use]
pub fn hide_auth_information(auth_header_value: &str) -> String {
    let sanitized: String =
        auth_header_value.chars().filter(|character| !character.is_control()).collect();
    let mut parts = sanitized.split(' ');
    let auth_type = parts.next().unwrap_or_default();
    let Some(token) = parts.next() else {
        return "[hidden]".to_string();
    };
    if token.chars().count() < 20 {
        return format!("{auth_type} [hidden]");
    }
    let prefix: String = token.chars().take(4).collect();
    format!("{auth_type} {prefix}[hidden]")
}

/// Strip `user:pass@` (or `user@`) that appears right after a URL scheme in
/// any message text, e.g. `… https://user:pass@host/pkg …` →
/// `… https://host/pkg …`. A registry configured as `https://user:pass@host/`
/// would otherwise leak its embedded basic-auth into a fetch error or a retry
/// log line.
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

/// Make untrusted, URL-bearing text safe to print or log: strip every
/// control character, then redact inline `user:pass@` credentials
/// ([`redact_url_credentials`]). Used for registry URLs and network-error
/// messages alike — both can carry basic-auth or escape sequences from an
/// untrusted `.npmrc` / `--registry` (or a `reqwest` error that echoes the
/// request URL back), which must not leak credentials or inject terminal
/// output via raw escapes / `\r` / `\n`.
///
/// The order is load-bearing: a control character inside the userinfo
/// (`user:pass\r@host`) would split the authority across the redaction
/// scan, and removing it afterwards would rejoin the credentials into the
/// output.
#[must_use]
pub fn redact_and_sanitize(text: &str) -> String {
    let sanitized = sanitize_control_characters(text);
    redact_url_credentials(&sanitized)
}

/// Make a URL safe for user-visible output without exposing credentials,
/// query parameters, fragments, or terminal control characters.
/// Malformed URLs are replaced entirely because their authority cannot be
/// redacted reliably.
#[must_use]
pub fn redact_url_for_display(url: &str) -> String {
    let sanitized = sanitize_control_characters(url);
    let Ok(mut display) = reqwest::Url::parse(&sanitized) else {
        return "[hidden]".to_string();
    };
    if display.set_username("").is_err() || display.set_password(None).is_err() {
        return "[hidden]".to_string();
    }
    display.set_query(None);
    display.set_fragment(None);
    display.to_string()
}

fn sanitize_control_characters(text: &str) -> String {
    text.chars().filter(|character| !character.is_control()).collect()
}

/// [`redact_and_sanitize`] for text whose line breaks are worth keeping, such
/// as a subprocess's multi-line stderr.
///
/// Line breaks are kept only when doing so redacts exactly as much as
/// collapsing the text would: a newline can fall inside a `user:pass@`
/// authority — git echoes such a URL back verbatim, password included — and
/// redacting each line separately would leave the credentials split but
/// readable. Whenever the two disagree, the collapsed form wins.
#[must_use]
pub fn redact_and_sanitize_multiline(text: &str) -> String {
    let collapsed = redact_and_sanitize(text);
    let per_line = text.split('\n').map(redact_and_sanitize).collect::<Vec<_>>().join("\n");
    if per_line.replace('\n', "") == collapsed { per_line } else { collapsed }
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
