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
//! installed on the resolve's [`AuthHeaders`](pacquet_network::AuthHeaders)
//! runs that classification at the real auth-selection point, selects the
//! pnpr-managed credential (never a client-forwarded one), and records the
//! route into a [`Footprint`]. The footprint's [`Footprint::digest`] is
//! the per-resolution private key the cache layer will gate on.

use std::{
    collections::BTreeSet,
    fmt,
    sync::{Arc, Mutex},
};

use pacquet_network::{MetadataCacheScope, UpstreamRouteHook, nerf_dart};
use reqwest::header::AUTHORIZATION;
use sha2::{Digest, Sha256};
use wax::{Glob, Program};

use crate::{
    config::{Config, PublicRoute, UplinkConfig},
    policy::{AccessList, Identity, PackagePolicies},
};

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
    /// alias the caller is authorized to use.
    Proxied { alias: String, generation: u64 },
}

/// The cache-namespace identity of a private route: a key input plus
/// (for the cache layer) an authorization gate. The key input is what
/// gets HMAC'd into the cache key; identical inputs from different
/// callers who share the same access collapse to one shared entry.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum PrivateAccessDescriptor {
    /// Proxied route via a pnpr-managed upstream alias. Rotating the
    /// alias bumps `generation`, moving future hits to a new namespace.
    Alias { alias: String, generation: u64 },
    /// pnpr-hosted route, gated by re-running the named package access
    /// policy for the caller.
    Hosted { policy_id: String },
}

impl PrivateAccessDescriptor {
    /// The stable, collision-resistant bytes that go into the cache-key
    /// HMAC. `\0` separates fields so distinct shapes can't alias (an
    /// alias literally named `hosted` can't collide with a hosted
    /// policy of the same text).
    fn key_input(&self) -> String {
        match self {
            PrivateAccessDescriptor::Alias { alias, generation } => {
                format!("alias\0{alias}\0{generation}")
            }
            PrivateAccessDescriptor::Hosted { policy_id } => format!("hosted\0{policy_id}"),
        }
    }

    /// The metadata-namespace id for this single descriptor: an HMAC over
    /// its [`Self::key_input`] keyed with the server `secret`, so the
    /// on-disk private-metadata mirror path is not correlatable offline.
    /// Distinct from [`Footprint::digest`], which combines *all* of a
    /// resolution's descriptors — metadata is fetched one route at a time,
    /// so each fetch keys on its own descriptor.
    fn digest_id(&self, secret: &[u8]) -> String {
        hex(&hmac_sha256(secret, self.key_input().as_bytes()))
    }
}

/// The HMAC digest namespacing an uplink's private cache (packuments and
/// tarballs), identical to the metadata-mirror descriptor id for the same
/// `(uplink, generation)`. Keyed by the server `secret` so the on-disk path
/// reveals neither the uplink name nor its generation, and so a path-unsafe
/// uplink name (`..`, `/`) can never escape the cache root — the digest is
/// hex.
#[must_use]
pub(crate) fn uplink_cache_digest(uplink: &str, generation: u64, secret: &[u8]) -> String {
    PrivateAccessDescriptor::Alias { alias: uplink.to_string(), generation }.digest_id(secret)
}

/// The set of private routes a single resolve actually touched, paired
/// with the descriptor selected for each. Accumulated during resolution
/// through the [`RouteHook`]; consumed afterwards to decide how the
/// resolution may be cached.
///
/// An empty footprint means the resolution is fully public and shareable
/// globally; a non-empty one is keyed by its private access descriptors.
#[derive(Debug, Default, Clone)]
pub struct Footprint {
    descriptors: BTreeSet<PrivateAccessDescriptor>,
}

impl Footprint {
    pub(crate) fn add(&mut self, descriptor: PrivateAccessDescriptor) {
        self.descriptors.insert(descriptor);
    }

    /// Whether the resolution touched no private data and may be cached
    /// under the global, auth-excluded key.
    #[must_use]
    pub fn is_public(&self) -> bool {
        self.descriptors.is_empty()
    }

    /// The private-key component for this footprint: an HMAC over the
    /// sorted union of its descriptors, keyed with the server `secret`
    /// so the key is not correlatable offline. `None` for a public
    /// footprint (no private descriptors), in which case the cache uses
    /// the global auth-excluded key.
    #[must_use]
    pub fn digest(&self, secret: &[u8]) -> Option<String> {
        if self.descriptors.is_empty() {
            return None;
        }
        let mut message = String::new();
        for descriptor in &self.descriptors {
            message.push_str(&descriptor.key_input());
            message.push('\n');
        }
        Some(hex(&hmac_sha256(secret, message.as_bytes())))
    }

    /// Whether every private descriptor in this footprint is still
    /// authorized for `identity` under the current route context.
    #[must_use]
    pub(crate) fn allows(&self, context: &RouteContext, identity: &Identity) -> bool {
        self.descriptors.iter().all(|descriptor| context.allows_descriptor(identity, descriptor))
    }
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
    /// Nerf-darted origin of every configured uplink (access-bearing or a
    /// plain mirror), forming the uplink half of the fetch allowlist. A
    /// plain mirror needs no credential, so it has no [`ResolvedAlias`]; it
    /// is still a configured registry pnpr may fetch from anonymously.
    uplink_origins: Vec<String>,
    /// Package access policy, used to decide whether a pnpr-hosted route
    /// is public (admits everyone) or private, and to gate hosted hits.
    policies: PackagePolicies,
}

#[derive(Debug, Clone)]
struct RouteMatcher {
    /// Nerf-darted registry prefix this rule applies to, or `None` for
    /// any registry.
    origin: Option<String>,
    package: Option<Glob<'static>>,
}

#[derive(Clone)]
struct ResolvedAlias {
    name: String,
    generation: u64,
    registry: String,
    /// Nerf-darted upstream origin the alias serves. Routing is by origin
    /// alone — an uplink credential covers every package on its registry.
    origin: String,
    /// The uplink registry's URL scheme (`https`/`http`). The credential is
    /// attached only to a fetch of this same scheme: nerf-darting strips the
    /// scheme from [`Self::origin`], so without this check an `http://host`
    /// fetch would match an `https://host` uplink and send its token in clear.
    scheme: String,
    /// The fully-formed `Authorization` header value the alias sends
    /// upstream (`Bearer ...` / `Basic ...`).
    authorization: String,
    /// Which pnpr callers may select this alias.
    access: AccessList,
}

impl fmt::Debug for ResolvedAlias {
    /// Redacts [`Self::authorization`] — it carries the uplink's server-owned
    /// upstream credential, which must never reach a log line or panic dump.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ResolvedAlias")
            .field("name", &self.name)
            .field("generation", &self.generation)
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
        // The official npm registry is a built-in public route, so it is both
        // allowlisted and classified public without any operator config (and
        // ahead of any uplink credential for the same origin — public wins).
        let public_routes = std::iter::once(RouteMatcher::npmjs())
            .chain(config.route_policy.public.iter().filter_map(RouteMatcher::from_public_route))
            .collect();
        // Proxied-route credentials come from `uplinks:` entries that declare
        // an `access:` policy. They are matched by registry origin and exposed
        // to clients at `/~<name>/`.
        let aliases = config
            .uplinks
            .iter()
            .filter_map(|(name, uplink)| ResolvedAlias::from_uplink(name, uplink))
            .collect();
        let uplink_origins =
            config.uplinks.values().filter_map(|uplink| nerf_prefix(&uplink.url)).collect();
        Self {
            hosted_origin,
            public_routes,
            aliases,
            uplink_origins,
            policies: config.policies.clone(),
        }
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
            // A fetch to pnpr's own `/~<uplink>/` endpoint addresses that
            // uplink, not a hosted package (a package name can never begin
            // with `~`). Authorized callers resolve through the uplink;
            // everyone else — and an unknown uplink — gets an anonymous
            // fetch the endpoint itself rejects, rather than falling through
            // to the hosted-package policy.
            if let Some(rest) = fetch.strip_prefix(hosted)
                && let Some(uplink) = rest.strip_prefix('~').and_then(|rest| rest.split('/').next())
                && !uplink.is_empty()
            {
                return match self
                    .aliases
                    .iter()
                    .find(|alias| alias.name == uplink && alias.access.allows(identity))
                {
                    Some(alias) => RouteClass::Proxied {
                        alias: alias.name.clone(),
                        generation: alias.generation,
                    },
                    None => RouteClass::Public,
                };
            }
            return self.classify_hosted(identity, package);
        }

        if let Some(alias) = self.select_alias(identity, &fetch)
            && scheme_of(url) == Some(alias.scheme.as_str())
        {
            // Scheme must match the uplink's: nerf-darting strips it, so an
            // `http://host` fetch would otherwise be handed an `https://host`
            // uplink's server-owned credential and leak it in cleartext. A
            // scheme mismatch falls through to an anonymous public fetch (which
            // fails closed upstream if the resource is actually private).
            return RouteClass::Proxied { alias: alias.name.clone(), generation: alias.generation };
        }

        RouteClass::Public
    }

    fn is_public_route(&self, fetch: &str, package: Option<&str>) -> bool {
        self.public_routes.iter().any(|route| route.matches(fetch, package))
    }

    /// Whether pnpr is permitted to fetch from `url`'s registry at all. The
    /// allowlist is the union of every configured route: the built-in npm
    /// host, operator-declared public routes, configured uplink origins, and
    /// pnpr's own origin (which serves its hosted packages and `/~<uplink>/`
    /// endpoints). A client `registry`/`namedRegistries` matching none of
    /// these is rejected before any server-side fetch — the resolver's SSRF
    /// boundary — so there is no "unknown registry" to resolve anonymously.
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
        if self
            .public_routes
            .iter()
            .any(|route| route.origin.as_deref().is_some_and(|origin| fetch.starts_with(origin)))
        {
            return true;
        }
        self.uplink_origins.iter().any(|origin| fetch.starts_with(origin))
    }

    /// A pnpr-hosted route is public when its package access policy
    /// admits an anonymous caller; otherwise it is private and gated by
    /// re-running that policy for the caller.
    fn classify_hosted(&self, identity: &Identity, package: Option<&str>) -> RouteClass {
        let Some(package) = package else {
            // A non-package fetch against pnpr itself carries no private
            // package data to key.
            return RouteClass::Public;
        };
        let access = self.policies.for_package(package).access;
        if access.allows(&Identity::Anonymous) {
            return RouteClass::Public;
        }
        if access.allows(identity) {
            RouteClass::Hosted { policy_id: package.to_string() }
        } else {
            // The caller can't read this hosted package: classify it as an
            // anonymous public fetch with no managed credential, which the
            // hosted-serving endpoint re-checks and rejects, so it never
            // matches a private hosted entry.
            RouteClass::Public
        }
    }

    fn select_alias(&self, identity: &Identity, fetch: &str) -> Option<&ResolvedAlias> {
        self.aliases
            .iter()
            .find(|alias| fetch.starts_with(&alias.origin) && alias.access.allows(identity))
    }

    pub(crate) fn allows_descriptor(
        &self,
        identity: &Identity,
        descriptor: &PrivateAccessDescriptor,
    ) -> bool {
        match descriptor {
            PrivateAccessDescriptor::Alias { alias, generation } => {
                // Reuse the cached resolution only if `identity` would *select*
                // this exact alias+generation for its origin — the first
                // authorized alias [`Self::select_alias`] returns there — not
                // merely one the caller is authorized for. With overlapping
                // uplink access (several aliases on one origin a caller can
                // use), an authorization-only check could replay a lockfile
                // routed through a different `/~<uplink>/` endpoint than this
                // caller resolves through. A since-removed alias (`find` →
                // `None`) or a rotated generation also fails closed here.
                self.aliases.iter().find(|candidate| candidate.name == alias.as_str()).is_some_and(
                    |candidate| {
                        self.select_alias(identity, &candidate.origin).is_some_and(|selected| {
                            selected.name == alias.as_str() && selected.generation == *generation
                        })
                    },
                )
            }
            PrivateAccessDescriptor::Hosted { policy_id } => {
                self.policies.for_package(policy_id).access.allows(identity)
            }
        }
    }

    /// The upstream registry an authorized caller reaches through the
    /// `/~<uplink>/` endpoint, used to reverse an endpoint tarball URL back to
    /// its upstream when verifying an input lockfile. Returns `None` when the
    /// uplink is unknown or the caller is not authorized for it.
    pub(crate) fn uplink_registry(&self, identity: &Identity, uplink: &str) -> Option<String> {
        self.aliases
            .iter()
            .find(|candidate| candidate.name == uplink && candidate.access.allows(identity))
            .map(|candidate| candidate.registry.clone())
    }
}

/// Nerf-darted origin of the official npm registry, the built-in public route.
const NPMJS_ORIGIN: &str = "//registry.npmjs.org/";

impl RouteMatcher {
    /// The built-in public route: the official npm registry, host-level (no
    /// package glob, so scoped and unscoped packages alike are public). An
    /// anonymous fetch returns only public content — a private scoped package
    /// `404`s — so a successfully-resolved npmjs route is public and globally
    /// shareable. Prepended to the operator-declared routes in
    /// [`RouteContext::from_config`], so it is allowlisted and public without
    /// any config, and ahead of any uplink credential for the same origin.
    fn npmjs() -> Self {
        Self { origin: Some(NPMJS_ORIGIN.to_string()), package: None }
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
        Some(Self { origin, package })
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
    /// Build a proxied-route alias from a `uplinks:` entry. An uplink
    /// participates in route classification only when it declares both an
    /// `access:` policy and a resolved `Authorization` credential; routing is
    /// by registry origin, so no package glob is attached.
    fn from_uplink(name: &str, uplink: &UplinkConfig) -> Option<Self> {
        let access = uplink.access.clone()?;
        let authorization =
            uplink.headers.get(AUTHORIZATION).and_then(|value| value.to_str().ok())?.to_string();
        Some(Self {
            name: name.to_string(),
            generation: uplink.generation,
            registry: uplink.url.clone(),
            origin: nerf_prefix(&uplink.url)?,
            scheme: scheme_of(&uplink.url)?.to_string(),
            authorization,
            access,
        })
    }
}

/// The [`UpstreamRouteHook`] pnpr installs on a resolve's
/// [`AuthHeaders`](pacquet_network::AuthHeaders). Every metadata/tarball
/// fetch routes through [`UpstreamRouteHook::authorize`], which classifies
/// the route, records it into the shared [`Footprint`], and returns the
/// pnpr-managed credential (never a client-forwarded one).
pub struct RouteHook {
    context: Arc<RouteContext>,
    identity: Identity,
    footprint: Arc<Mutex<Footprint>>,
    /// HMAC secret keying the per-descriptor metadata namespace
    /// ([`MetadataCacheScope::Private`]); the same server secret the
    /// resolution cache keys private footprints with.
    secret: Arc<[u8]>,
}

impl fmt::Debug for RouteHook {
    /// Redacts [`Self::secret`] — the descriptor-HMAC key must never reach a
    /// log line or panic dump, or the private namespace becomes correlatable.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RouteHook")
            .field("context", &self.context)
            .field("identity", &self.identity)
            .field("footprint", &self.footprint)
            .field("secret", &"<redacted>")
            .finish()
    }
}

impl RouteHook {
    #[must_use]
    pub fn new(
        context: Arc<RouteContext>,
        identity: Identity,
        footprint: Arc<Mutex<Footprint>>,
        secret: Arc<[u8]>,
    ) -> Self {
        Self { context, identity, footprint, secret }
    }
}

impl UpstreamRouteHook for RouteHook {
    fn authorize(&self, url: &str, package: Option<&str>) -> Option<String> {
        match self.context.classify(&self.identity, url, package) {
            RouteClass::Public => None,
            RouteClass::Hosted { policy_id } => {
                self.record(PrivateAccessDescriptor::Hosted { policy_id });
                // Hosted packages are served by pnpr itself; no upstream
                // credential is involved.
                None
            }
            RouteClass::Proxied { alias, generation } => {
                let authorization = self
                    .context
                    .aliases
                    .iter()
                    .find(|candidate| candidate.name == alias)
                    .map(|candidate| candidate.authorization.clone());
                self.record(PrivateAccessDescriptor::Alias { alias, generation });
                authorization
            }
        }
    }

    fn metadata_scope(&self, url: &str, package: Option<&str>) -> MetadataCacheScope {
        // Read-only classification — this must not record into the
        // footprint (`authorize` already does, at the real fetch point).
        match self.context.classify(&self.identity, url, package) {
            RouteClass::Public => MetadataCacheScope::Public,
            RouteClass::Hosted { policy_id } => MetadataCacheScope::Private {
                descriptor_id: PrivateAccessDescriptor::Hosted { policy_id }
                    .digest_id(&self.secret),
            },
            RouteClass::Proxied { alias, generation } => MetadataCacheScope::Private {
                descriptor_id: PrivateAccessDescriptor::Alias { alias, generation }
                    .digest_id(&self.secret),
            },
        }
    }
}

impl RouteHook {
    fn record(&self, descriptor: PrivateAccessDescriptor) {
        self.footprint.lock().expect("footprint poisoned").add(descriptor);
    }
}

/// Whether a registry/dependency/tarball spec carries inline
/// `user:pass@host` (or `user@host`) credentials. Such URLs must be
/// rejected before any fetch: pnpr must not turn a client-embedded
/// credential into upstream Basic auth, treat it as a cache identity, or
/// store it in a shared cache. Specs without a `scheme://` (a semver
/// range, a scoped name, `npm:`/`workspace:` aliases) never match.
#[must_use]
pub fn url_has_inline_credentials(spec: &str) -> bool {
    let Some((_, after_scheme)) = spec.split_once("://") else {
        return false;
    };
    let authority = after_scheme.split(['/', '?', '#']).next().unwrap_or(after_scheme);
    match authority.rsplit_once('@') {
        Some((userinfo, _)) => !userinfo.is_empty(),
        None => false,
    }
}

/// Strip an inline `user:pass@`/`user@` userinfo from a `scheme://` URL,
/// returning the credential-free form. A tarball URL taken from an untrusted
/// upstream `dist.tarball` must never be emitted to a client or written to a
/// shared cache with embedded credentials; a genuinely public tarball is
/// anonymously fetchable, so the stripped URL still works. Returns the input
/// unchanged when there is no `scheme://` authority or no userinfo.
#[must_use]
pub(crate) fn strip_url_credentials(url: &str) -> String {
    let Some((scheme, after)) = url.split_once("://") else {
        return url.to_string();
    };
    let authority_end = after.find(['/', '?', '#']).unwrap_or(after.len());
    let (authority, rest) = after.split_at(authority_end);
    match authority.rsplit_once('@') {
        Some((_, host)) => format!("{scheme}://{host}{rest}"),
        None => url.to_string(),
    }
}

/// Sanitize an upstream `dist.tarball` URL for public emission: drop inline
/// userinfo (via [`strip_url_credentials`]) **and** any query string or
/// fragment, where a registry could carry a signed-URL / tokenized credential
/// (`?X-Amz-Signature=…`, `?token=…`). A genuinely public tarball is fetched by
/// its bare path and verified by SRI regardless, so the sanitized URL still
/// works; one that truly needed a token was never a public route and must be
/// configured as an uplink (whose tarballs route through `/~<uplink>/`, keeping
/// the token server-side). Unlike a client-supplied direct-tarball spec — whose
/// query is the caller's own intent — this is untrusted upstream metadata.
#[must_use]
pub(crate) fn sanitize_registry_tarball_url(url: &str) -> String {
    let no_creds = strip_url_credentials(url);
    match no_creds.split_once(['?', '#']) {
        Some((base, _)) => base.to_string(),
        None => no_creds,
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
/// or proxied-uplink route. Path-preserving (`//host/base/`), unlike a bare
/// host: a pnpr served under a path prefix (`https://host/pnpr/`) still
/// recognizes its own `/pnpr/~<uplink>/` endpoints, and a public/uplink route
/// declared for `https://host/base/` does not also match a sibling
/// `https://host/other/` path on the same host.
fn nerf_prefix(url: &str) -> Option<String> {
    let nerfed = nerf_dart(url);
    if nerfed.is_empty() { None } else { Some(nerfed) }
}

/// The URL scheme (`https`, `http`, ...), i.e. the segment before `://`. `None`
/// for a value with no scheme.
fn scheme_of(url: &str) -> Option<&str> {
    url.split_once("://").map(|(scheme, _)| scheme)
}

/// Whether a nerf-darted key (`//host/path/`) has a `.` or `..` path segment,
/// which could escape a path-scoped prefix match in [`RouteContext::allows_registry`].
fn contains_dot_segment(nerfed: &str) -> bool {
    nerfed.split('/').any(|segment| segment == "." || segment == "..")
}

/// HMAC-SHA256 (RFC 2104) over the workspace's audited `sha2`, so the
/// descriptor digest needs no extra crypto dependency. Verified against
/// the RFC 4231 test vectors in the unit tests.
fn hmac_sha256(key: &[u8], message: &[u8]) -> [u8; 32] {
    const BLOCK: usize = 64;
    let mut block_key = [0u8; BLOCK];
    if key.len() > BLOCK {
        let digest = Sha256::digest(key);
        block_key[..digest.len()].copy_from_slice(&digest);
    } else {
        block_key[..key.len()].copy_from_slice(key);
    }
    let mut inner = Sha256::new();
    let mut outer = Sha256::new();
    let mut inner_pad = [0u8; BLOCK];
    let mut outer_pad = [0u8; BLOCK];
    for index in 0..BLOCK {
        inner_pad[index] = block_key[index] ^ 0x36;
        outer_pad[index] = block_key[index] ^ 0x5c;
    }
    inner.update(inner_pad);
    inner.update(message);
    outer.update(outer_pad);
    outer.update(inner.finalize());
    outer.finalize().into()
}

fn hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

#[cfg(test)]
mod tests;
