//! Authorization-aware route classification for the resolution cache.
//!
//! pnpr's resolution cache stores a whole resolved lockfile keyed by the
//! resolution inputs, with auth deliberately excluded from the key. That
//! is safe only for resolutions that touched no private data. This module
//! decides, for every metadata/tarball fetch a resolve performs, whether
//! its **route** is public or private — without a second request — so a
//! public resolution can be shared globally while a private one is keyed
//! by the *private access descriptor* that produced it.
//!
//! Privacy is a property of the fetch route (registry + package +
//! configured rules), not of whether the request carried a credential:
//!
//! * scoped names can be public (`@babel/core` on npmjs), and
//! * unscoped names can be private (a corporate default registry).
//!
//! [`RouteContext::classify`] maps one fetch to a [`RouteClass`]. The [`RouteHook`]
//! installed on the resolve's [`AuthHeaders`](pnpm_network::AuthHeaders)
//! runs that classification at the real auth-selection point, selects the
//! pnpr-managed credential (never a client-forwarded one), and records the
//! route into a [`Footprint`]. The footprint's [`Footprint::digest`] is
//! the per-resolution private key the cache layer will gate on.

pub use url_credentials::{
    sanitize_registry_tarball_url, strip_url_credentials, url_has_inline_credentials,
};

pub use route_hook::RouteHook;

pub use footprint::{
    Footprint, PrivateAccessDescriptor, credential_digest, headers_credential_digest,
    upstream_cache_digest,
};

mod url_credentials;
use url_credentials::scheme_of;

mod route_hook;

mod footprint;

use std::{
    collections::BTreeSet,
    fmt,
    sync::{Arc, Mutex},
};

use indexmap::IndexMap;
use pnpm_network::{MetadataCacheScope, UpstreamRouteHook, nerf_dart};
use reqwest::header::{AUTHORIZATION, HeaderMap};
use sha2::{Digest, Sha256};
use wax::{Glob, Program};

use pnpr_config::{Config, PublicRoute, UpstreamConfig};
use pnpr_policy::{AccessList, Identity, PackageRules};
use pnpr_registry::{ConcreteKind, Ecosystem, Registries, Resolved};

/// The classification of a single fetch route.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RouteClass {
    /// Public route — fetched anonymously and shareable globally. The
    /// built-in unscoped-npmjs route, an operator-declared public route,
    /// or a pnpr-hosted package whose access policy admits everyone.
    Public,
    /// A package hosted by pnpr itself whose access policy is private.
    /// Gated by re-running that policy for the caller; `policy_id`
    /// identifies the access-policy rule that produced the entry.
    Hosted { policy_id: String },
    /// A proxied upstream route served with a pnpr-managed credential
    /// alias the caller is authorized to use. [`credential_digest`] is a hash
    /// of the upstream's `Authorization`, so rotating the credential changes it
    /// (see [`credential_digest`]).
    Proxied { alias: String, credential_digest: String },
}

/// Everything [`RouteContext::classify`] needs, resolved once from the server
/// [`Config`] and reused across every fetch in a resolve.
#[derive(Debug, Clone)]
pub struct RouteContext {
    /// Nerf-darted origin of this pnpr service (from `public_url`). A
    /// fetch whose URL falls under it is a pnpr-hosted route.
    hosted_origin: Option<String>,
    /// Public routes, matched by nerf-darted registry prefix and/or package
    /// glob. Always begins with the built-in official-npm route
    /// ([`RouteMatcher::npmjs`]), followed by the operator-declared ones.
    public_routes: Vec<RouteMatcher>,
    /// pnpr-managed upstream credential aliases, in declared order.
    aliases: Vec<ResolvedAlias>,
    /// Nerf-darted origin of every configured upstream (access-bearing or a
    /// plain mirror), forming the upstream half of the fetch allowlist. A
    /// plain mirror needs no credential, so it has no [`ResolvedAlias`]; it
    /// is still a configured registry pnpr may fetch from anonymously.
    upstream_origins: Vec<String>,
    /// The registry routing graph, used to resolve a path-less fetch to the
    /// concrete registry that serves it — the same dispatch the serving
    /// endpoints use, so classification and serving can't disagree about a
    /// package's origin.
    registries: Registries,
    /// Each hosted registry's `packages:` rules, used to decide whether a
    /// pnpr-hosted route is public (its effective access admits everyone)
    /// or private, and to gate hosted cache hits for the caller.
    hosted_rules: IndexMap<String, PackageRules>,
    /// Each upstream registry's `packages:` rules. Alias selection is
    /// per-package-aware: a caller the upstream's effective access denies
    /// for a name is never handed the server-owned credential for it, so a
    /// fresh resolve fails closed exactly where the serving endpoint would
    /// deny the read. (Cache *replay* granularity stays registry-scoped:
    /// the alias descriptor covers every name the alias resolves, shared
    /// among callers the registry-level `access:` admits.)
    upstream_rules: IndexMap<String, PackageRules>,
}

#[derive(Debug, Clone)]
struct RouteMatcher {
    /// Nerf-darted registry prefix this rule applies to, or `None` for
    /// any registry.
    origin: Option<String>,
    package: Option<Glob<'static>>,
    /// Whether the route allowlists `https` fetches only. Set on the built-in
    /// routes, whose hosts are public and serve TLS — see
    /// [`RouteContext::allows_registry`].
    https_only: bool,
}

#[derive(Clone)]
struct ResolvedAlias {
    name: String,
    /// [`credential_digest`] of [`Self::authorization`]: the credential epoch
    /// this alias's private cache entries are keyed by. Changes when the
    /// upstream credential rotates.
    credential_digest: String,
    registry: String,
    /// Nerf-darted upstream origin the alias serves. Routing is by origin
    /// alone — an upstream credential covers every package on its registry.
    origin: String,
    /// The upstream registry's URL scheme (`https`/`http`). The credential is
    /// attached only to a fetch of this same scheme: nerf-darting strips the
    /// scheme from [`Self::origin`], so without this check an `http://host`
    /// fetch would match an `https://host` upstream and send its token in clear.
    scheme: String,
    /// The fully-formed `Authorization` header value the alias sends
    /// upstream (`Bearer ...` / `Basic ...`).
    authorization: String,
    /// Which pnpr callers may select this alias.
    access: AccessList,
}

impl fmt::Debug for ResolvedAlias {
    /// Redacts [`Self::authorization`] — it carries the upstream's server-owned
    /// upstream credential, which must never reach a log line or panic dump.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ResolvedAlias")
            .field("name", &self.name)
            .field("credential_digest", &self.credential_digest)
            .field("registry", &self.registry)
            .field("origin", &self.origin)
            .field("scheme", &self.scheme)
            .field("authorization", &"<redacted>")
            .field("access", &self.access)
            .finish()
    }
}

impl RouteContext {
    /// Resolve route-classification inputs from the server config.
    #[must_use]
    pub fn from_config(config: &Config) -> Self {
        let hosted_origin = nerf_prefix(&config.public_url);
        let mut registries = config.registries.clone();
        for name in config.upstreams.keys() {
            registries.ensure_upstream(name);
        }
        // The registries pnpm itself routes to without configuration are
        // built-in public routes, so they are both allowlisted and classified
        // public without any operator config (and ahead of any upstream
        // credential for the same origin — public wins).
        let public_routes = [RouteMatcher::npmjs(), RouteMatcher::jsr()]
            .into_iter()
            .chain(config.route_policy.public.iter().filter_map(RouteMatcher::from_public_route))
            .collect();
        // Proxied-route credentials come from `upstreams:` entries that declare
        // an `access:` policy. They are matched by registry origin and exposed
        // to clients at `/~<name>/`.
        let aliases = config
            .upstreams
            .iter()
            .filter_map(|(name, upstream)| ResolvedAlias::from_upstream(name, upstream))
            .collect();
        let upstream_origins =
            config.upstreams.values().filter_map(|upstream| nerf_prefix(&upstream.url)).collect();
        let hosted_rules = config
            .hosted
            .iter()
            .map(|(name, hosted)| (name.clone(), hosted.rules.clone()))
            .collect();
        let upstream_rules = config
            .upstreams
            .iter()
            .map(|(name, upstream)| (name.clone(), upstream.rules.clone()))
            .collect();
        Self {
            hosted_origin,
            public_routes,
            aliases,
            upstream_origins,
            registries,
            hosted_rules,
            upstream_rules,
        }
    }

    #[must_use]
    pub fn is_only_ecosystem(&self, ecosystem: Ecosystem) -> bool {
        self.registries.is_only_ecosystem(ecosystem)
    }

    /// See [`Registries::base_path`].
    #[must_use]
    pub fn base_path(&self, ecosystem: Ecosystem) -> String {
        self.registries.base_path(ecosystem)
    }

    /// Whether the upstream registry's effective per-package access admits
    /// `identity` for `package`. A non-package fetch and an upstream with no
    /// rules entry (a programmatically folded one) gate at the registry
    /// level only, which alias selection already checked.
    fn upstream_admits(&self, registry: &str, identity: &Identity, package: Option<&str>) -> bool {
        let (Some(package), Some(rules)) = (package, self.upstream_rules.get(registry)) else {
            return true;
        };
        rules.for_package(package).access.allows(identity)
    }

    /// The descriptor package qualifier for a proxied fetch: `Some(package)`
    /// only when the upstream's rules explicitly refine this name's access,
    /// so the cache descriptor re-checks that refinement on replay. Names
    /// the registry-level gate alone covers stay registry-scoped, keeping
    /// the common footprint one descriptor per alias.
    fn alias_package_qualifier(&self, alias: &str, package: Option<&str>) -> Option<String> {
        let (package, rules) = (package?, self.upstream_rules.get(alias)?);
        rules.for_package(package).access_is_explicit.then(|| package.to_string())
    }

    /// Classify a single fetch to `url` for `package` (`None` for a
    /// non-package fetch), for `identity`. Precedence follows the RFC:
    /// public wins and suppresses auth; then pnpr-hosted; then an
    /// authorized proxied alias; otherwise an anonymous public fetch (no
    /// managed credential). The fetch allowlist ([`Self::allows_registry`])
    /// runs first at the request boundary, so a route reaching here is a
    /// configured registry: an anonymous fall-through either succeeds —
    /// proving the content is public and globally shareable — or fails
    /// closed upstream (`401`/`403`) when it actually needed a credential
    /// the caller is not authorized for.
    #[must_use]
    pub fn classify(&self, identity: &Identity, url: &str, package: Option<&str>) -> RouteClass {
        let fetch = nerf_dart(url);
        if fetch.is_empty() {
            return RouteClass::Public;
        }

        if self.is_public_route(&fetch, package) {
            return RouteClass::Public;
        }

        if let Some(hosted) = self.hosted_origin.as_deref()
            && fetch.starts_with(hosted)
        {
            return self.classify_own_origin(identity, &fetch, hosted, package);
        }

        if let Some(alias) = self.select_alias(identity, &fetch, package)
            && scheme_of(url) == Some(alias.scheme.as_str())
        {
            // Scheme must match the upstream's: nerf-darting strips it, so an
            // `http://host` fetch would otherwise be handed an `https://host`
            // upstream's server-owned credential and leak it in cleartext. A
            // scheme mismatch falls through to an anonymous public fetch (which
            // fails closed upstream if the resource is actually private).
            return RouteClass::Proxied {
                alias: alias.name.clone(),
                credential_digest: alias.credential_digest.clone(),
            };
        }

        RouteClass::Public
    }

    /// Classify a fetch aimed at pnpr's own origin.
    ///
    /// A fetch to pnpr's own `/~<name>/` endpoint addresses that registry
    /// directly (a package name can never begin with `~`): an access-bearing
    /// upstream resolves through its alias for authorized callers, a hosted
    /// registry through its own rules. Everyone else — and an unknown name —
    /// gets an anonymous fetch the endpoint itself rejects, rather than
    /// falling through to another registry's policy.
    fn classify_own_origin(
        &self,
        identity: &Identity,
        fetch: &str,
        hosted: &str,
        package: Option<&str>,
    ) -> RouteClass {
        // Spelled out rather than built from `base_path`: this wants a path
        // segment with a trailing slash, not the `/<ecosystem>` prefix that
        // URL building uses.
        let npm_endpoint = if self.registries.is_only_ecosystem(Ecosystem::Npm) {
            hosted.to_string()
        } else {
            format!("{hosted}npm/")
        };
        if let Some(registry) = addressed_registry_segment(fetch, &npm_endpoint) {
            return self.classify_addressed(identity, registry, package);
        }

        // A path-less fetch resolves through the graph's default registry —
        // the same dispatch the serving endpoints use — to the one concrete
        // registry that serves this package.
        let Some(package) = package else {
            // A non-package fetch against pnpr itself carries no private
            // package data to key.
            return RouteClass::Public;
        };
        match self.registries.resolve_default(pnpr_registry::Ecosystem::Npm, package) {
            Resolved::Concrete { registry, kind: ConcreteKind::Hosted } => {
                self.classify_hosted(identity, registry, Some(package))
            }
            Resolved::Concrete { registry, kind: ConcreteKind::Upstream } => {
                self.classify_upstream(identity, registry, package)
            }
            // Unclaimed or no default registry: the endpoint answers
            // not-found, so there is no private content to key.
            Resolved::Unclaimed | Resolved::UnknownRegistry => RouteClass::Public,
        }
    }

    /// Classify a fetch that names its registry through `/~<name>/`.
    fn classify_addressed(
        &self,
        identity: &Identity,
        registry: &str,
        package: Option<&str>,
    ) -> RouteClass {
        let Some(registry) = self.registries.addressed(registry, Ecosystem::Npm) else {
            return RouteClass::Public;
        };
        if let Some(alias) = self.authorized_alias(identity, registry) {
            // Per-package refinement: a name the upstream's rules deny this
            // caller gets no credential — the anonymous fetch fails closed at
            // the endpoint, matching serving.
            if self.upstream_admits(&alias.name, identity, package) {
                return RouteClass::Proxied {
                    alias: alias.name.clone(),
                    credential_digest: alias.credential_digest.clone(),
                };
            }
            return RouteClass::Public;
        }
        if self.hosted_rules.contains_key(registry) {
            return self.classify_hosted(identity, registry, package);
        }
        RouteClass::Public
    }

    /// Classify a package served by an upstream registry reached as the
    /// default. A public upstream source (no alias) is an anonymous public
    /// fetch; an unauthorized caller falls through to one the endpoint fails
    /// closed on.
    fn classify_upstream(&self, identity: &Identity, registry: &str, package: &str) -> RouteClass {
        // Per-package refinement — see `classify_addressed`.
        let admitted = self
            .authorized_alias(identity, registry)
            .filter(|alias| self.upstream_admits(&alias.name, identity, Some(package)));
        match admitted {
            Some(alias) => RouteClass::Proxied {
                alias: alias.name.clone(),
                credential_digest: alias.credential_digest.clone(),
            },
            None => RouteClass::Public,
        }
    }

    fn authorized_alias(&self, identity: &Identity, registry: &str) -> Option<&ResolvedAlias> {
        self.aliases.iter().find(|alias| alias.name == registry && alias.access.allows(identity))
    }

    fn is_public_route(&self, fetch: &str, package: Option<&str>) -> bool {
        self.public_routes.iter().any(|route| route.matches(fetch, package))
    }

    /// Whether pnpr is permitted to fetch from `url`'s registry at all. The
    /// allowlist is the union of every configured route: the built-in public
    /// hosts, operator-declared public routes, configured upstream origins, and
    /// pnpr's own origin (which serves its hosted packages and `/~<name>/`
    /// endpoints). A client `registry`/`namedRegistries` matching none of
    /// these is rejected before any server-side fetch — the resolver's SSRF
    /// boundary — so there is no "unknown registry" to resolve anonymously.
    ///
    /// Every hop of a redirect is re-checked here too, so a route that admits
    /// cleartext admits a downgrade to it. The built-in hosts therefore
    /// allowlist `https` alone; an operator's own routes and upstreams keep
    /// whatever scheme they are declared with, which on an internal network is
    /// legitimately plain HTTP.
    #[must_use]
    pub fn allows_registry(&self, url: &str) -> bool {
        let fetch = nerf_dart(url);
        if fetch.is_empty() {
            return false;
        }
        // A `.`/`..` path segment can slip past a path-scoped prefix match
        // (`//host/base/../admin` starts_with `//host/base/` yet resolves to
        // `//host/admin`), escaping the allowlist. Registries never use them,
        // so any dot-segment fails closed.
        if contains_dot_segment(&fetch) {
            return false;
        }
        if self.hosted_origin.as_deref().is_some_and(|hosted| fetch.starts_with(hosted)) {
            return true;
        }
        if self.public_routes.iter().any(|route| route.allowlists(&fetch, scheme_of(url))) {
            return true;
        }
        self.upstream_origins.iter().any(|origin| fetch.starts_with(origin))
    }

    /// A pnpr-hosted route is public when the hosted registry's effective
    /// access for the package admits an anonymous caller; otherwise it is
    /// private and gated by re-running that registry's rules for the caller.
    /// This is the same effective-access lookup the serving gate
    /// (`hosted_gate`) admits with, so classification and serving cannot
    /// disagree about *whether* a caller may read a name — the serving
    /// tiers only vary the denial's shape (mask vs. 401/403), and every
    /// denial classifies as an anonymous public fetch the endpoint fails
    /// closed on. The descriptor is registry-qualified — the same
    /// `name@version` on two hosted registries is two different packages,
    /// so their cache entries must never share a key.
    fn classify_hosted(
        &self,
        identity: &Identity,
        registry: &str,
        package: Option<&str>,
    ) -> RouteClass {
        let Some(package) = package else {
            // A non-package fetch against pnpr itself carries no private
            // package data to key.
            return RouteClass::Public;
        };
        let Some(rules) = self.hosted_rules.get(registry) else {
            return RouteClass::Public;
        };
        let access = rules.for_package(package).access;
        if access.allows(&Identity::Anonymous) {
            return RouteClass::Public;
        }
        if access.allows(identity) {
            RouteClass::Hosted { policy_id: hosted_policy_id(registry, package) }
        } else {
            // The caller can't read this hosted package: classify it as an
            // anonymous public fetch with no managed credential, which the
            // hosted-serving endpoint re-checks and rejects, so it never
            // matches a private hosted entry.
            RouteClass::Public
        }
    }

    fn select_alias(
        &self,
        identity: &Identity,
        fetch: &str,
        package: Option<&str>,
    ) -> Option<&ResolvedAlias> {
        self.aliases.iter().find(|alias| {
            fetch.starts_with(&alias.origin)
                && alias.access.allows(identity)
                && self.upstream_admits(&alias.name, identity, package)
        })
    }

    pub(crate) fn allows_descriptor(
        &self,
        identity: &Identity,
        descriptor: &PrivateAccessDescriptor,
    ) -> bool {
        match descriptor {
            PrivateAccessDescriptor::Alias { alias, credential_digest, package } => {
                // Reuse the cached resolution only if `identity` would *select*
                // this exact alias for its origin — the first authorized alias
                // [`Self::select_alias`] returns there — and its credential
                // still hashes to the same digest. Not merely an alias the
                // caller is authorized for: with overlapping upstream access
                // (several aliases on one origin a caller can use), an
                // authorization-only check could replay a lockfile routed
                // through a different `/~<name>/` endpoint than this caller
                // resolves through. A since-removed alias (`find` → `None`) or
                // a rotated credential also fails closed here. A descriptor
                // carrying a package qualifier (recorded when the upstream's
                // rules explicitly refine that name) re-checks the per-package
                // gate through `select_alias`, so cache replay is exactly as
                // strict as a fresh resolve; unqualified descriptors gate at
                // the registry level, shared among the callers the upstream's
                // `access:` admits.
                self.aliases.iter().find(|candidate| candidate.name == alias.as_str()).is_some_and(
                    |candidate| {
                        self.select_alias(identity, &candidate.origin, package.as_deref())
                            .is_some_and(|selected| {
                                selected.name == alias.as_str()
                                    && selected.credential_digest == *credential_digest
                            })
                    },
                )
            }
            PrivateAccessDescriptor::Hosted { policy_id } => {
                // The id is registry-qualified (see `hosted_policy_id`); a
                // descriptor that doesn't parse — including one written by a
                // pre-registry-scoped build — fails closed and re-resolves.
                match policy_id.split_once('\0') {
                    Some((registry, package)) => self
                        .hosted_rules
                        .get(registry)
                        .is_some_and(|rules| rules.for_package(package).access.allows(identity)),
                    None => false,
                }
            }
        }
    }

    /// The upstream registry an authorized caller reaches through the
    /// `/~<name>/` endpoint, used to reverse an endpoint tarball URL back to
    /// its upstream when verifying an input lockfile. Returns `None` when the
    /// upstream is unknown or the caller is not authorized for it.
    #[must_use]
    pub fn upstream_registry(&self, identity: &Identity, upstream: &str) -> Option<String> {
        self.aliases
            .iter()
            .find(|candidate| candidate.name == upstream && candidate.access.allows(identity))
            .map(|candidate| candidate.registry.clone())
    }
}

/// The registry-qualified id of one hosted package's access decision, stored
/// in [`PrivateAccessDescriptor::Hosted`]. `\0` separates the components so a
/// registry name (URL-safe, never NUL) can't alias into a package name.
fn hosted_policy_id(registry: &str, package: &str) -> String {
    format!("{registry}\0{package}")
}

/// Nerf-darted origin of the official npm registry, a built-in public route.
const NPMJS_ORIGIN: &str = "//registry.npmjs.org/";

/// Nerf-darted origin of the JSR registry's npm compatibility layer, the
/// registry pnpm's built-in `@jsr` scope route points at.
const JSR_ORIGIN: &str = "//npm.jsr.io/";

impl RouteMatcher {
    /// The built-in public route: the official npm registry, host-level (no
    /// package glob, so scoped and unscoped packages alike are public). An
    /// anonymous fetch returns only public content — a private scoped package
    /// `404`s — so a successfully-resolved npmjs route is public and globally
    /// shareable. Prepended to the operator-declared routes in
    /// [`RouteContext::from_config`], so it is allowlisted and public without
    /// any config, and ahead of any upstream credential for the same origin.
    fn npmjs() -> Self {
        Self { origin: Some(NPMJS_ORIGIN.to_string()), package: None, https_only: true }
    }

    /// The other built-in public route: JSR, which every pnpm client routes
    /// the `@jsr` scope to without configuring anything, so a graph holding a
    /// JSR dependency resolves through a default server. It hosts no private
    /// content, so the reasoning in [`Self::npmjs`] applies unchanged.
    fn jsr() -> Self {
        Self { origin: Some(JSR_ORIGIN.to_string()), package: None, https_only: true }
    }

    /// Build a matcher from an operator-declared public route, failing
    /// closed. An *omitted* `registry`/`package` field means "match any" —
    /// the intended wildcard — but a field that is *present yet unparsable*
    /// (a typo'd registry URL or glob) drops the whole rule (`None`) rather
    /// than collapsing to a `None` field that [`Self::matches`] would read as
    /// match-any. A typo must narrow matching, never widen a scoped public
    /// route into a match-all that leaks private metadata onto the public
    /// path.
    fn from_public_route(route: &PublicRoute) -> Option<Self> {
        let origin = match route.registry.as_deref() {
            None => None,
            Some(registry) => Some(nerf_prefix(registry).or_else(|| {
                tracing::warn!(registry, "ignoring public route with an unparsable registry URL");
                None
            })?),
        };
        let package = match route.package.as_deref() {
            None => None,
            Some(pattern) => Some(compile_glob(pattern).or_else(|| {
                tracing::warn!(pattern, "ignoring public route with an invalid package glob");
                None
            })?),
        };
        Some(Self { origin, package, https_only: false })
    }

    /// Whether this route puts `fetch` (nerf-darted, so scheme-less) on the
    /// allowlist when reached over `scheme`. A package-scoped route with no
    /// registry allowlists nothing: it narrows an origin another rule already
    /// admits.
    fn allowlists(&self, fetch: &str, scheme: Option<&str>) -> bool {
        self.origin.as_deref().is_some_and(|origin| fetch.starts_with(origin))
            && (!self.https_only
                || scheme.is_some_and(|scheme| scheme.eq_ignore_ascii_case("https")))
    }

    fn matches(&self, fetch: &str, package: Option<&str>) -> bool {
        let origin_ok = self.origin.as_deref().is_none_or(|origin| fetch.starts_with(origin));
        let package_ok = self
            .package
            .as_ref()
            .is_none_or(|glob| package.is_some_and(|name| glob.is_match(name)));
        origin_ok && package_ok
    }
}

impl ResolvedAlias {
    /// Build a proxied-route alias from a `upstreams:` entry. An upstream
    /// participates in route classification only when it declares both an
    /// `access:` policy and a resolved `Authorization` credential; routing is
    /// by registry origin, so no package glob is attached.
    fn from_upstream(name: &str, upstream: &UpstreamConfig) -> Option<Self> {
        let access = upstream.access.clone()?;
        let authorization =
            upstream.headers.get(AUTHORIZATION).and_then(|value| value.to_str().ok())?.to_string();
        Some(Self {
            name: name.to_string(),
            credential_digest: credential_digest(&authorization),
            registry: upstream.url.clone(),
            origin: nerf_prefix(&upstream.url)?,
            scheme: scheme_of(&upstream.url)?.to_string(),
            authorization,
            access,
        })
    }
}

/// Compile a package glob, returning `None` for an invalid pattern rather
/// than failing the whole resolve. [`RouteMatcher::from_public_route`] turns
/// that `None` into a dropped (never-matching) rule, so an operator typo
/// narrows matching instead of opening a private route up.
fn compile_glob(pattern: &str) -> Option<Glob<'static>> {
    Glob::new(pattern).ok().map(Glob::into_owned)
}

/// Nerf-dart a registry URL down to its host-only origin
/// (`//host[:port]/`), the prefix every fetch under it shares. `None`
/// for an unparsable URL.
/// The nerf-darted registry prefix used to match fetches to a hosted, public,
/// or proxied-upstream route. Path-preserving (`//host/base/`), unlike a bare
/// host: a pnpr served under a path prefix (`https://host/pnpr/`) still
/// recognizes its own `/pnpr/~<name>/` endpoints, and a public/upstream route
/// declared for `https://host/base/` does not also match a sibling
/// `https://host/other/` path on the same host.
fn nerf_prefix(url: &str) -> Option<String> {
    let nerfed = nerf_dart(url);
    if nerfed.is_empty() { None } else { Some(nerfed) }
}

/// The URL scheme (`https`, `http`, ...), i.e. the segment before `://`. `None`
/// for a value with no scheme.
/// The registry a `/~<name>/` endpoint path addresses, if it names one.
fn addressed_registry_segment<'a>(fetch: &'a str, npm_endpoint: &str) -> Option<&'a str> {
    let rest = fetch.strip_prefix(npm_endpoint)?;
    let registry = rest.strip_prefix('~')?.split('/').next()?;
    (!registry.is_empty()).then_some(registry)
}

/// Whether a nerf-darted key (`//host/path/`) has a `.` or `..` path segment,
/// which could escape a path-scoped prefix match in [`RouteContext::allows_registry`].
fn contains_dot_segment(nerfed: &str) -> bool {
    nerfed.split('/').any(|segment| segment == "." || segment == "..")
}

#[cfg(test)]
mod tests;
