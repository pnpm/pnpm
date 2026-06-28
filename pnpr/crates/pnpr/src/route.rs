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
    config::{Config, UplinkConfig},
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
    /// A private/unknown route for which the caller has no usable
    /// pnpr-managed credential or hosted authorization. Resolved
    /// anonymously as a *non-shareable* miss; never written to the
    /// global public cache.
    Unknown,
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

/// The set of private routes a single resolve actually touched, paired
/// with the descriptor selected for each. Accumulated during resolution
/// through the [`RouteHook`]; consumed afterwards to decide how the
/// resolution may be cached.
///
/// An empty footprint with no unknown-private touch means the resolution
/// is fully public and shareable globally.
#[derive(Debug, Default, Clone)]
pub struct Footprint {
    descriptors: BTreeSet<PrivateAccessDescriptor>,
    /// A private/unknown route with no usable descriptor was fetched, so
    /// the resolution carries private data that cannot be safely keyed —
    /// it must not be shared at all.
    touched_unknown_private: bool,
}

impl Footprint {
    pub(crate) fn add(&mut self, descriptor: PrivateAccessDescriptor) {
        self.descriptors.insert(descriptor);
    }

    fn mark_unknown_private(&mut self) {
        self.touched_unknown_private = true;
    }

    /// Whether the resolution touched no private data and may be cached
    /// under the global, auth-excluded key.
    #[must_use]
    pub fn is_public(&self) -> bool {
        self.descriptors.is_empty() && !self.touched_unknown_private
    }

    /// Whether the resolution may be cached at all. A resolution that
    /// touched an unknown-private route carries private data with no
    /// descriptor to key it, so it can be served only to the caller that
    /// produced it (i.e. not shared) — Part 2 treats this as
    /// non-cacheable.
    #[must_use]
    pub fn is_shareable(&self) -> bool {
        !self.touched_unknown_private
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
        !self.touched_unknown_private
            && self
                .descriptors
                .iter()
                .all(|descriptor| context.allows_descriptor(identity, descriptor))
    }
}

/// Everything [`RouteContext::classify`] needs, resolved once from the server
/// [`Config`] and reused across every fetch in a resolve.
#[derive(Debug, Clone)]
pub struct RouteContext {
    /// Built-in route: unscoped packages on `registry.npmjs.org` are
    /// public. Operators can disable this for a conservative deployment.
    npmjs_unscoped_public: bool,
    /// Nerf-darted origin of this pnpr service (from `public_url`). A
    /// fetch whose URL falls under it is a pnpr-hosted route.
    hosted_origin: Option<String>,
    /// Operator-declared public routes, matched by nerf-darted registry
    /// prefix and/or package glob.
    public_routes: Vec<RouteMatcher>,
    /// pnpr-managed upstream credential aliases, in declared order.
    aliases: Vec<ResolvedAlias>,
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
    /// The fully-formed `Authorization` header value the alias sends
    /// upstream (`Bearer …` / `Basic …`).
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
        let public_routes = config
            .route_policy
            .public
            .iter()
            .map(|route| RouteMatcher {
                origin: route.registry.as_deref().and_then(nerf_prefix),
                package: route.package.as_deref().and_then(compile_glob),
            })
            .collect();
        // Proxied-route credentials come from `uplinks:` entries that declare
        // an `access:` policy. They are matched by registry origin and exposed
        // to clients at `/~<name>/`.
        let aliases = config
            .uplinks
            .iter()
            .filter_map(|(name, uplink)| ResolvedAlias::from_uplink(name, uplink))
            .collect();
        Self {
            npmjs_unscoped_public: config.route_policy.npmjs_unscoped_public,
            hosted_origin,
            public_routes,
            aliases,
            policies: config.policies.clone(),
        }
    }

    /// Classify a single fetch to `url` for `package` (`None` for a
    /// non-package fetch), for `identity`. Precedence follows the RFC:
    /// public wins and suppresses auth; then pnpr-hosted; then an
    /// authorized proxied alias; otherwise unknown/private.
    #[must_use]
    pub fn classify(&self, identity: &Identity, url: &str, package: Option<&str>) -> RouteClass {
        let fetch = nerf_dart(url);
        if fetch.is_empty() {
            return RouteClass::Unknown;
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
            // everyone else — and an unknown uplink — fails closed rather
            // than falling through to the hosted-package policy.
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
                    None => RouteClass::Unknown,
                };
            }
            return self.classify_hosted(identity, package);
        }

        if let Some(alias) = self.select_alias(identity, &fetch) {
            return RouteClass::Proxied { alias: alias.name.clone(), generation: alias.generation };
        }

        RouteClass::Unknown
    }

    fn is_public_route(&self, fetch: &str, package: Option<&str>) -> bool {
        if self.npmjs_unscoped_public
            && origin_of(fetch) == Some("registry.npmjs.org")
            && package.is_none_or(|name| !name.starts_with('@'))
        {
            return true;
        }
        self.public_routes.iter().any(|route| route.matches(fetch, package))
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
            // The caller can't read this hosted package, so it must not
            // match any private hosted entry; fail closed.
            RouteClass::Unknown
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
                self.aliases.iter().any(|candidate| {
                    candidate.name == alias.as_str()
                        && candidate.generation == *generation
                        && candidate.access.allows(identity)
                })
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

impl RouteMatcher {
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
            RouteClass::Unknown => {
                self.footprint.lock().expect("footprint poisoned").mark_unknown_private();
                None
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
            RouteClass::Unknown => MetadataCacheScope::Bypass,
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

/// Compile a package glob, dropping (rather than failing the whole
/// resolve over) an invalid pattern. An operator typo therefore makes a
/// route *more* restrictive (the rule never matches) rather than opening
/// a private route up.
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

/// The `host[:port]` of a nerf-darted key (`//host[:port]/path/`).
fn origin_of(nerfed: &str) -> Option<&str> {
    nerfed.strip_prefix("//")?.split('/').next().filter(|host| !host.is_empty())
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
