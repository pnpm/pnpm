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
    hide_auth_information, redact_and_sanitize, redact_and_sanitize_multiline, redact_npm_auth_key,
    redact_url_credentials, redact_url_for_display,
};
pub use route_hook::{MetadataCacheScope, UpstreamRouteHook};
pub use url::{base64_encode, base64_encode_bytes, is_url_secure_for_credentials, nerf_dart};

use crate::token_helper::{TokenHelperRunner, execute_token_helper, run_token_helper_command};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fmt,
    sync::{Arc, Mutex, OnceLock},
};

pub const DEFAULT_REGISTRY_SCOPE: &str = "@";

pub type AuthHeadersByScope = BTreeMap<String, BTreeMap<String, String>>;

/// Bag of `Authorization` header values keyed by the nerf-darted form
/// of each registry URL. Ecosystem-specific configuration readers normalize
/// their credentials into this request-facing form and share it across HTTP
/// calls made during install.
///
/// Construct via [`AuthHeaders::from_parts`], [`AuthHeaders::from_creds_map`],
/// [`AuthHeaders::from_map`], or [`AuthHeaders::default`] (empty). Look up via
/// [`AuthHeaders::for_url`].
/// Memo of resolved `tokenHelper` results keyed by `scope + map key`.
type TokenHelperCache = Arc<Mutex<HashMap<String, Arc<OnceLock<Option<String>>>>>>;

pub(super) type ScopedAuthMap = HashMap<String, HashMap<String, AuthEntry>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CredentialOrigin {
    Http,
    Https,
    SchemeLess,
}

#[derive(Default, Clone)]
struct TransportSecurity {
    require_secure: bool,
    insecure_uris: HashSet<String>,
}

impl TransportSecurity {
    fn allows(
        &self,
        key: &str,
        entry_allow_insecure: bool,
        is_secure: bool,
        origin: CredentialOrigin,
    ) -> bool {
        if is_secure {
            return true;
        }
        if self.require_secure || origin == CredentialOrigin::Https {
            return false;
        }
        entry_allow_insecure
            || self.insecure_uris
                .iter()
                .any(|prefix| key == prefix || key.starts_with(prefix))
    }
}

#[derive(Default, Clone)]
pub(super) struct AuthMap {
    pub(super) by_uri: HashMap<String, AuthEntry>,
    pub(super) scoped_by_scope: ScopedAuthMap,
    pub(super) max_parts: usize,
    pub(super) max_scoped_parts_by_scope: HashMap<String, usize>,
}

impl AuthMap {
    pub(super) fn is_empty(&self) -> bool {
        self.by_uri.is_empty() && self.scoped_by_scope.is_empty()
    }

    pub(super) fn max_scoped_parts(&self, scope: &str) -> usize {
        self.max_scoped_parts_by_scope
            .get(scope)
            .copied()
            .unwrap_or(0)
    }

    pub(super) fn get_scoped(&self, scope: &str, key: &str) -> Option<&AuthEntry> {
        self.scoped_by_scope
            .get(scope)
            .and_then(|map| map.get(key))
    }

    pub(super) fn get_uri(&self, key: &str) -> Option<&AuthEntry> {
        self.by_uri.get(key)
    }

    fn populate_by_scope(&self, result: &mut AuthHeadersByScope) {
        for (uri, entry) in &self.by_uri {
            if let AuthKind::Header(value) = &entry.kind {
                result
                    .entry(uri.clone())
                    .or_default()
                    .insert(DEFAULT_REGISTRY_SCOPE.to_owned(), value.clone());
            }
        }
        for (scope, scoped_by_uri) in &self.scoped_by_scope {
            for (registry_uri, entry) in scoped_by_uri {
                if let AuthKind::Header(value) = &entry.kind {
                    result
                        .entry(registry_uri.clone())
                        .or_default()
                        .insert(scope.clone(), value.clone());
                }
            }
        }
    }

    fn mark_insecure(&mut self, prefix: &str) {
        mark_map_insecure(&mut self.by_uri, prefix);
        for scoped_by_uri in self.scoped_by_scope.values_mut() {
            mark_map_insecure(scoped_by_uri, prefix);
        }
    }
}

#[derive(Default, Clone)]
pub struct AuthHeaders {
    default_auth: AuthMap,
    http_auth: AuthMap,
    /// Server-side route hook. When set, it owns every auth lookup and
    /// the client-forwarded credentials above are ignored. See
    /// [`UpstreamRouteHook`].
    route_hook: Option<Arc<dyn UpstreamRouteHook>>,
    transport_security: TransportSecurity,
    /// Set iff any entry is an [`AuthKind::TokenHelper`]. Surfaced in the
    /// [`fmt::Debug`] output (never the values) so a resolve trace shows
    /// at a glance whether any helper is configured. The lookup hot path
    /// touches the resolution cache only on a `TokenHelper` match, so a
    /// map of only baked headers pays nothing regardless.
    has_token_helpers: bool,
    token_helpers: TokenHelpers,
}

#[derive(Default, Clone)]
struct TokenHelpers {
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
pub(crate) struct AuthEntry {
    pub(crate) kind: AuthKind,
    pub(crate) allow_insecure: bool,
    pub(crate) origin: CredentialOrigin,
}

#[derive(Clone)]
pub(crate) enum AuthKind {
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
            .field("by_uri", &self.default_auth.by_uri.len())
            .field("http_by_uri", &self.http_auth.by_uri.len())
            .field("scoped_by_scope", &self.default_auth.scoped_by_scope.len())
            .field("http_scoped_by_scope", &self.http_auth.scoped_by_scope.len())
            .field("insecure_uris", &self.transport_security.insecure_uris.len())
            .field("has_token_helpers", &self.has_token_helpers)
            .field("route_hook", &self.route_hook.is_some())
            .field("require_secure_transport", &self.transport_security.require_secure)
            .finish_non_exhaustive()
    }
}

impl AuthHeaders {
    /// Restrict every credential lookup to TLS or loopback URLs, including
    /// lookups made by shared fetchers. This restriction survives cloning.
    #[must_use]
    pub fn with_secure_transport(mut self) -> Self {
        self.transport_security.require_secure = true;
        self
    }

    /// Whether no configured credential or route hook can provide
    /// authorization for any URL.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.default_auth.is_empty()
            && self.http_auth.is_empty()
            && self.route_hook.is_none()
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
        let origin = if url.starts_with("http://") {
            CredentialOrigin::Http
        } else if url.starts_with("https://") {
            CredentialOrigin::Https
        } else {
            CredentialOrigin::SchemeLess
        };
        let allow_insecure =
            origin == CredentialOrigin::Http && !is_url_secure_for_credentials(url);
        if allow_insecure {
            self.transport_security.insecure_uris.insert(uri.clone());
            self.transport_security.insecure_uris.insert(normalize_auth_key(url.to_owned()));
        }
        let entry = AuthEntry { kind: AuthKind::Header(header), allow_insecure, origin };
        let target_auth = if origin == CredentialOrigin::Http {
            &mut self.http_auth
        } else {
            &mut self.default_auth
        };
        target_auth.max_parts = target_auth.max_parts.max(uri.split('/').count());
        target_auth.by_uri.insert(uri, entry);
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
                scoped_by_uri
                    .entry(registry_uri)
                    .or_default()
                    .insert(scope, value);
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
        let (by_uri_entries, http_by_uri_entries, insecure_uris) =
            collect_uri_entries(by_uri, token_helper_by_uri);
        let (scoped_entries, http_scoped_entries) =
            collect_scoped_entries(scoped_by_uri, token_helper_scoped_by_uri);

        Self::from_entry_parts(
            by_uri_entries,
            http_by_uri_entries,
            scoped_entries,
            http_scoped_entries,
            insecure_uris,
        )
    }

    /// Assemble the lookup indices from already-wrapped [`AuthEntry`]
    /// maps: the per-scope longest-key counts, the top-level
    /// `max_parts`, and the `has_token_helpers` gate.
    fn from_entry_parts(
        by_uri: HashMap<String, AuthEntry>,
        http_by_uri: HashMap<String, AuthEntry>,
        scoped_by_uri: ScopedAuthMap,
        http_scoped_by_uri: ScopedAuthMap,
        insecure_uris: HashSet<String>,
    ) -> Self {
        let (default_auth, has_helpers_default) = build_auth_map(by_uri, scoped_by_uri);
        let (http_auth, has_helpers_http) = build_auth_map(http_by_uri, http_scoped_by_uri);
        let has_token_helpers = has_helpers_default || has_helpers_http;
        AuthHeaders {
            default_auth,
            http_auth,
            route_hook: None,
            transport_security: TransportSecurity { require_secure: false, insecure_uris },
            has_token_helpers,
            token_helpers: TokenHelpers::default(),
        }
    }

    /// Override the runner used to execute `tokenHelper` commands.
    #[must_use]
    pub fn with_token_helper_runner(mut self, runner: TokenHelperRunner) -> Self {
        self.token_helpers.token_helper_runner = Some(runner);
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
                    scoped_by_uri
                        .entry(uri.clone())
                        .or_default()
                        .insert(scope, value);
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
        self.http_auth.populate_by_scope(&mut result);
        self.default_auth.populate_by_scope(&mut result);
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

    /// Explicitly allow sending credentials over cleartext HTTP to this host
    /// or nerf-dart key (e.g. when configured in trusted user settings).
    pub fn allow_insecure_host(&mut self, url_or_nerf: &str) {
        let nerfed = if url_or_nerf.starts_with("http://") || url_or_nerf.starts_with("https://") {
            nerf_dart(url_or_nerf)
        } else {
            normalize_auth_key(url_or_nerf.to_owned())
        };
        if nerfed.is_empty() {
            return;
        }
        self.transport_security.insecure_uris.insert(nerfed.clone());

        self.mark_insecure_entries(&nerfed);
    }

    fn mark_insecure_entries(&mut self, prefix: &str) {
        self.default_auth.mark_insecure(prefix);
        self.http_auth.mark_insecure(prefix);
    }
}

fn build_auth_map(
    by_uri: HashMap<String, AuthEntry>,
    scoped_by_uri: ScopedAuthMap,
) -> (AuthMap, bool) {
    let (scoped_by_scope, max_scoped_parts_by_scope, has_scoped_helpers) =
        index_scoped_entries(scoped_by_uri);
    let has_helpers = has_scoped_helpers
        || by_uri
            .values()
            .any(|entry| matches!(entry.kind, AuthKind::TokenHelper(_)));
    let max_parts = max_parts_of(&by_uri);
    (AuthMap { by_uri, scoped_by_scope, max_parts, max_scoped_parts_by_scope }, has_helpers)
}

fn mark_map_insecure(map: &mut HashMap<String, AuthEntry>, prefix: &str) {
    for (uri, entry) in map.iter_mut() {
        if entry.origin != CredentialOrigin::Https && (uri == prefix || uri.starts_with(prefix)) {
            entry.allow_insecure = true;
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

#[cfg(test)]
mod tests;

mod builder;
use builder::{
    collect_scoped_entries, collect_uri_entries, index_scoped_entries, max_parts_of,
    split_scoped_auth_key,
};

mod lookup;
mod redaction;
mod route_hook;
mod url;
use url::ParsedUrl;
