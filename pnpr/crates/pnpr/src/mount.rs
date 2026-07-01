//! Registry mounts: the routing graph and its static validation.
//!
//! A **registry mount** is an addressable npm-registry surface exposed at
//! `https://<pnpr>/~<mount>/`. There are two concrete kinds — a pnpr-hosted
//! registry and a single-origin upstream registry — plus one composite, a
//! **router**, that maps package-name patterns to a single concrete source.
//!
//! The model is governed by one invariant: **provenance is declared, never
//! inferred.** A package resolves to exactly one declared concrete origin, and
//! no configuration can express a cross-origin fall-through. A router's first
//! matching route is authoritative; later routes are never consulted, and a
//! matched source's "not found" or "unavailable" is final. There is no
//! existence-based fallback, no mirror group, and no multi-endpoint failover.
//!
//! Because routing is first-match-in-order, route order is load-bearing: a
//! misordered router is the one way a configuration mistake could silently send
//! a private scope to a public origin. [`Mounts::validate`] rejects that class
//! at config load (and reload) — shadowed/unreachable routes, duplicate
//! patterns, and sources that are unknown, self-referential, or not concrete.
//! The check is static because [`PackagePattern`]'s coverage relation is
//! decidable for this deliberately small glob language.

use crate::package_name::PackageName;
use indexmap::IndexMap;
use std::fmt;

/// A package-name routing pattern.
///
/// Deliberately a small, **decidable** language so [`Self::covers`] can decide
/// statically whether one pattern matches a superset of another — the property
/// [`Mounts::validate`] relies on to detect shadowed routes. A general glob
/// (`wax`) would make coverage undecidable, so router patterns are restricted
/// to these four shapes; an unrecognized wildcard is a parse error rather than
/// a silently-narrowing literal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PackagePattern {
    /// `**` — every package name.
    All,
    /// `@*/*` — every scoped package, any scope.
    AnyScoped,
    /// `@<scope>/*` — every package in one scope. Stores the scope without its
    /// leading `@`.
    Scope(String),
    /// A literal package name (`foo` or `@scope/foo`).
    Exact(String),
}

impl PackagePattern {
    /// Parse a router pattern. Rejects any `*` that is not one of the three
    /// recognized wildcard shapes (`**`, `@*/*`, `@<scope>/*`), and any
    /// remaining literal that is not a well-formed package name — so an
    /// unsupported glob or a typo like `@acme` (meaning `@acme/*`) fails
    /// loudly instead of being read as a literal name that silently never
    /// matches and lets the scope fall through to a later route.
    pub fn parse(pattern: &str) -> Result<Self, MountConfigError> {
        let invalid = || MountConfigError::InvalidPattern { pattern: pattern.to_string() };
        if pattern.is_empty() {
            return Err(invalid());
        }
        if pattern == "**" {
            return Ok(PackagePattern::All);
        }
        if pattern == "@*/*" {
            return Ok(PackagePattern::AnyScoped);
        }
        if let Some(scope) = pattern.strip_prefix('@').and_then(|rest| rest.strip_suffix("/*")) {
            // A single, concrete scope: `@acme/*`. The scope itself may not
            // contain a wildcard or a further path separator.
            if scope.is_empty() || scope.contains('*') || scope.contains('/') {
                return Err(invalid());
            }
            return Ok(PackagePattern::Scope(scope.to_string()));
        }
        // Anything left that still carries a `*` is an unsupported glob.
        if pattern.contains('*') {
            return Err(invalid());
        }
        if PackageName::parse(pattern).is_err() {
            return Err(MountConfigError::ExactPatternNotAName { pattern: pattern.to_string() });
        }
        Ok(PackagePattern::Exact(pattern.to_string()))
    }

    /// Whether this pattern matches `package`.
    #[must_use]
    pub fn matches(&self, package: &str) -> bool {
        match self {
            PackagePattern::All => true,
            // A scoped pattern matches only a well-formed `@scope/name`, never a
            // bare `@scope` with no name segment.
            PackagePattern::AnyScoped => scoped_name(package).is_some(),
            PackagePattern::Scope(scope) => {
                scoped_name(package).is_some_and(|(package_scope, _)| package_scope == scope)
            }
            PackagePattern::Exact(name) => name == package,
        }
    }

    /// Whether this pattern matches every package the `other` pattern matches
    /// (i.e. `self` ⊇ `other`). Decides route shadowing in [`Mounts::validate`].
    ///
    /// A union of earlier patterns can only shadow `other` through a single
    /// member: the one unbounded case — many `@<scope>/*` covering `@*/*` —
    /// would need every scope enumerated, which is impossible, so per-pattern
    /// coverage is sufficient for union coverage in this language.
    #[must_use]
    pub fn covers(&self, other: &PackagePattern) -> bool {
        use PackagePattern::{All, AnyScoped, Exact, Scope};
        match self {
            All => true,
            AnyScoped => match other {
                All => false,
                AnyScoped | Scope(_) => true,
                // Consistent with `matches`: an exact name is scoped only when
                // it is a well-formed `@scope/name`, never a bare `@scope`.
                Exact(name) => scoped_name(name).is_some(),
            },
            Scope(scope) => match other {
                Scope(other_scope) => other_scope == scope,
                Exact(name) => scoped_name(name).is_some_and(|(name_scope, _)| name_scope == scope),
                All | AnyScoped => false,
            },
            Exact(name) => matches!(other, Exact(other_name) if other_name == name),
        }
    }
}

impl fmt::Display for PackagePattern {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PackagePattern::All => f.write_str("**"),
            PackagePattern::AnyScoped => f.write_str("@*/*"),
            PackagePattern::Scope(scope) => write!(f, "@{scope}/*"),
            PackagePattern::Exact(name) => f.write_str(name),
        }
    }
}

/// The `(scope, name)` of a well-formed scoped package (`@acme/foo` →
/// `("acme", "foo")`), or `None` when it is unscoped or missing either segment
/// (`@acme`, `@/foo`, `@acme/`).
fn scoped_name(package: &str) -> Option<(&str, &str)> {
    let (scope, name) = package.strip_prefix('@')?.split_once('/')?;
    (!scope.is_empty() && !name.is_empty()).then_some((scope, name))
}

/// One router route: package-name patterns mapped to a single concrete source
/// mount. Evaluated in declared order; the first route with a matching pattern
/// is authoritative.
#[derive(Debug, Clone)]
pub struct Route {
    pub patterns: Vec<PackagePattern>,
    /// A [`MountKind::Hosted`] or [`MountKind::Upstream`] mount id — never
    /// another router (enforced by [`Mounts::validate`]).
    pub source: String,
}

impl Route {
    /// Whether any of this route's patterns matches `package`.
    fn matches(&self, package: &str) -> bool {
        self.patterns.iter().any(|pattern| pattern.matches(package))
    }
}

/// The routing role of a mount. The per-mount serving details (upstream URL,
/// credentials, access policy, org id) live in the `config` module; this
/// captures only what routing and validation need.
#[derive(Debug, Clone)]
pub enum MountKind {
    /// A pnpr-hosted registry: the authoritative origin for the packages it
    /// stores, and the only kind that accepts writes. Reads and writes are
    /// scoped to the mount's own storage namespace (its optional `org`).
    Hosted,
    /// Exactly one external origin. One URL, one credential generation, one
    /// cache namespace — not a chain and not a set of endpoints.
    Upstream,
    /// Maps package-name patterns to concrete mounts in declared order.
    Router { routes: Vec<Route> },
}

impl MountKind {
    fn is_concrete(&self) -> bool {
        matches!(self, MountKind::Hosted | MountKind::Upstream)
    }
}

/// The kind of a concrete (non-router) source a request resolved to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConcreteKind {
    Hosted,
    Upstream,
}

/// The outcome of resolving a request `(mount, package)` to a concrete origin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolved<'a> {
    /// Resolved to exactly one concrete source mount.
    Concrete { mount: &'a str, kind: ConcreteKind },
    /// A router matched no route for this package. The request is a definitive
    /// `404`; the router never falls through to another origin.
    NoRoute,
    /// The addressed mount id is not defined.
    UnknownMount,
}

/// The validated set of registry mounts plus the optional path-less default
/// target. Built and validated by the `config` module at load time.
#[derive(Debug, Default, Clone)]
pub struct Mounts {
    mounts: IndexMap<String, MountKind>,
    /// The mount the path-less base URL (`https://<pnpr>/`) aliases. `None`
    /// disables the path-less base entirely — clients must address a mount.
    default_target: Option<String>,
}

impl Mounts {
    #[must_use]
    pub fn new(mounts: IndexMap<String, MountKind>, default_target: Option<String>) -> Self {
        Self { mounts, default_target }
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.mounts.is_empty()
    }

    #[must_use]
    pub fn get(&self, mount: &str) -> Option<&MountKind> {
        self.mounts.get(mount)
    }

    #[must_use]
    pub fn default_target(&self) -> Option<&str> {
        self.default_target.as_deref()
    }

    /// Whether `mount` is a defined router.
    #[must_use]
    pub fn is_router(&self, mount: &str) -> bool {
        matches!(self.mounts.get(mount), Some(MountKind::Router { .. }))
    }

    /// Resolve a request addressed to `mount` for `package` to its single
    /// concrete origin. A concrete mount resolves to itself; a router resolves
    /// by first matching route (authoritatively — a non-matching router is
    /// [`Resolved::NoRoute`], never a fall-through).
    #[must_use]
    pub fn resolve<'a>(&'a self, mount: &str, package: &str) -> Resolved<'a> {
        let Some((mount_id, kind)) = self.mounts.get_key_value(mount) else {
            return Resolved::UnknownMount;
        };
        match kind {
            MountKind::Hosted => Resolved::Concrete { mount: mount_id, kind: ConcreteKind::Hosted },
            MountKind::Upstream => {
                Resolved::Concrete { mount: mount_id, kind: ConcreteKind::Upstream }
            }
            MountKind::Router { routes } => {
                let Some(route) = routes.iter().find(|route| route.matches(package)) else {
                    return Resolved::NoRoute;
                };
                // Validation guarantees every route source is a defined concrete
                // mount, so the lookup and the kind classification cannot miss.
                match self.mounts.get_key_value(&route.source) {
                    Some((source_id, MountKind::Hosted)) => {
                        Resolved::Concrete { mount: source_id, kind: ConcreteKind::Hosted }
                    }
                    Some((source_id, MountKind::Upstream)) => {
                        Resolved::Concrete { mount: source_id, kind: ConcreteKind::Upstream }
                    }
                    _ => Resolved::UnknownMount,
                }
            }
        }
    }

    /// Resolve a request to the path-less base (`https://<pnpr>/`) through the
    /// configured default target. With no default target the path-less base is
    /// disabled, so every package is [`Resolved::UnknownMount`].
    #[must_use]
    pub fn resolve_default<'a>(&'a self, package: &str) -> Resolved<'a> {
        match self.default_target.as_deref() {
            Some(target) => self.resolve(target, package),
            None => Resolved::UnknownMount,
        }
    }

    /// Validate the whole mount set, failing closed on any configuration that
    /// could route a private name to the wrong origin or leave a route dead.
    /// Run at config load and on reload.
    pub fn validate(&self) -> Result<(), MountConfigError> {
        if let Some(target) = &self.default_target
            && !self.mounts.contains_key(target)
        {
            return Err(MountConfigError::UndefinedDefaultTarget { target: target.clone() });
        }
        for (name, kind) in &self.mounts {
            if let MountKind::Router { routes } = kind {
                // A router with no routes can never match any package — every
                // request through it is a 404. That's only ever a config
                // mistake (a hosted/upstream mount was probably intended), so
                // reject it like an empty route.
                if routes.is_empty() {
                    return Err(MountConfigError::EmptyRouter { router: name.clone() });
                }
                self.validate_router(name, routes)?;
            }
        }
        Ok(())
    }

    fn validate_router(&self, router: &str, routes: &[Route]) -> Result<(), MountConfigError> {
        let mut seen_patterns: Vec<&PackagePattern> = Vec::new();
        for (index, route) in routes.iter().enumerate() {
            if route.patterns.is_empty() {
                return Err(MountConfigError::EmptyRoute { router: router.to_string(), index });
            }
            // The source must resolve to a defined concrete mount: an unknown
            // name, the router itself, or another router are all rejected, so a
            // route can only ever land on a real origin (no nesting, no cycles).
            if route.source == router {
                return Err(MountConfigError::SelfReferentialRouter { router: router.to_string() });
            }
            match self.mounts.get(&route.source) {
                None => {
                    return Err(MountConfigError::UnknownSource {
                        router: router.to_string(),
                        source: route.source.clone(),
                    });
                }
                Some(source) if !source.is_concrete() => {
                    return Err(MountConfigError::NonConcreteSource {
                        router: router.to_string(),
                        source: route.source.clone(),
                    });
                }
                Some(_) => {}
            }
            // A route is unreachable when every one of its patterns is already
            // covered by some pattern in an earlier route — the misordered-`**`
            // hazard and its general form. Reject it so a shadowed private route
            // is a startup error, not a silent public fall-through.
            if route
                .patterns
                .iter()
                .all(|pattern| seen_patterns.iter().any(|earlier| earlier.covers(pattern)))
            {
                return Err(MountConfigError::UnreachableRoute {
                    router: router.to_string(),
                    index,
                    source: route.source.clone(),
                });
            }
            for pattern in &route.patterns {
                // A pattern strictly narrower than an earlier route's pattern can
                // never match — every package it claims is already routed away.
                // The whole-route check above only fires when *all* of a route's
                // patterns are covered; catch the partial case here so one
                // shadowed pattern in an otherwise-reachable route can't silently
                // send a private package to the origin an earlier `**`/scope route
                // points at. Strict (`earlier != pattern`) so an exact repeat
                // stays the clearer `DuplicatePattern` below rather than a shadow.
                if let Some(earlier) = seen_patterns
                    .iter()
                    .find(|&&earlier| earlier != pattern && earlier.covers(pattern))
                {
                    return Err(MountConfigError::ShadowedPattern {
                        router: router.to_string(),
                        pattern: pattern.to_string(),
                        by: earlier.to_string(),
                    });
                }
                if seen_patterns.contains(&pattern) {
                    return Err(MountConfigError::DuplicatePattern {
                        router: router.to_string(),
                        pattern: pattern.to_string(),
                    });
                }
                seen_patterns.push(pattern);
            }
        }
        Ok(())
    }
}

/// A static mount-configuration defect. Surfaced by [`Mounts::validate`] and by
/// [`PackagePattern::parse`]; the `config` module turns it into an
/// `InvalidConfig` so a bad mount set fails server startup and config reload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MountConfigError {
    /// An unsupported wildcard in a router pattern.
    InvalidPattern { pattern: String },
    /// A wildcard-free router pattern that is not a well-formed package name,
    /// so it could never match any request.
    ExactPatternNotAName { pattern: String },
    /// `defaultTarget` names a mount that does not exist.
    UndefinedDefaultTarget { target: String },
    /// A router route has no patterns, so it can never match.
    EmptyRoute { router: String, index: usize },
    /// A router has no routes at all, so it can never match any package.
    EmptyRouter { router: String },
    /// A router route lists the router itself as its source.
    SelfReferentialRouter { router: String },
    /// A router route's source is not a defined mount.
    UnknownSource { router: String, source: String },
    /// A router route's source is another router, not a concrete mount.
    NonConcreteSource { router: String, source: String },
    /// Two router routes declare the same pattern.
    DuplicatePattern { router: String, pattern: String },
    /// A router route is fully shadowed by earlier routes and can never match.
    UnreachableRoute { router: String, index: usize, source: String },
    /// A single router pattern is strictly covered by an earlier route's
    /// pattern, so it can never match even though the rest of its route can.
    ShadowedPattern { router: String, pattern: String, by: String },
}

impl fmt::Display for MountConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MountConfigError::InvalidPattern { pattern } => write!(
                f,
                "unsupported router pattern {pattern:?}: use an exact name, `@scope/*`, `@*/*`, \
                 or `**`",
            ),
            MountConfigError::ExactPatternNotAName { pattern } => write!(
                f,
                "router pattern {pattern:?} is not a valid package name, so it can never match; \
                 to route every package in a scope use `@scope/*`",
            ),
            MountConfigError::UndefinedDefaultTarget { target } => {
                write!(f, "defaultTarget {target:?} is not a defined mount")
            }
            MountConfigError::EmptyRoute { router, index } => {
                write!(f, "router {router:?} route #{index} has no patterns", index = index + 1)
            }
            MountConfigError::EmptyRouter { router } => write!(
                f,
                "router {router:?} has no routes, so it can never match any package; add routes \
                 or remove the mount",
            ),
            MountConfigError::SelfReferentialRouter { router } => {
                write!(f, "router {router:?} lists itself as a route source")
            }
            MountConfigError::UnknownSource { router, source } => {
                write!(f, "router {router:?} route source {source:?} is not a defined mount")
            }
            MountConfigError::NonConcreteSource { router, source } => write!(
                f,
                "router {router:?} route source {source:?} is itself a router; a route must target \
                 a hosted or upstream mount",
            ),
            MountConfigError::DuplicatePattern { router, pattern } => {
                write!(f, "router {router:?} declares pattern {pattern:?} more than once")
            }
            MountConfigError::UnreachableRoute { router, index, source } => write!(
                f,
                "router {router:?} route #{index} (source {source:?}) is unreachable: earlier \
                 routes already match every package it would; reorder it before the routes that \
                 shadow it, or remove it",
                index = index + 1,
            ),
            MountConfigError::ShadowedPattern { router, pattern, by } => write!(
                f,
                "router {router:?} pattern {pattern:?} is unreachable: earlier pattern {by:?} \
                 already matches every package it would; reorder it before {by:?} or remove it",
            ),
        }
    }
}

impl std::error::Error for MountConfigError {}

#[cfg(test)]
mod tests;
