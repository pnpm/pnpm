//! Registry mounts: the routing graph and its static validation.
//!
//! A **registry mount** is an addressable npm-registry surface exposed at
//! `https://<pnpr>/~<mount>/`. There are two concrete kinds — a pnpr-hosted
//! registry and a single-origin upstream registry — plus one composite, a
//! **router**, an ordered list of concrete mounts behind one URL.
//!
//! The model is governed by one invariant: **provenance is declared, never
//! inferred.** Every concrete mount declares the package-name patterns it
//! serves — its namespace — and that namespace is enforced on the mount
//! itself, on every path to it: an unclaimed name is a definitive not-found
//! before storage or the upstream is consulted, whether the request came
//! through a router or addressed the mount directly. A router selects the
//! first listed source whose patterns claim the name — authoritatively. It can
//! order competing claims, but it can never assign a name to a mount that does
//! not claim it, so no configuration can express a cross-origin fall-through:
//! a selected source's "not found" or "unavailable" is final. There is no
//! existence-based fallback, no mirror group, and no multi-endpoint failover.
//!
//! Because selection is first-source-in-order, source order is load-bearing: a
//! misordered router is the one way a configuration mistake could silently send
//! a private scope to a public origin. [`Mounts::validate`] rejects that class
//! at config load (and reload) — shadowed/unreachable sources, duplicate
//! sources and patterns, and sources that are unknown, self-referential, or
//! not concrete. The check is static because [`PackagePattern`]'s coverage
//! relation is decidable for this deliberately small glob language.

use crate::package_name::PackageName;
use indexmap::IndexMap;
use std::fmt;

/// A package-name pattern: one member of a concrete mount's declared
/// namespace.
///
/// Deliberately a small, **decidable** language so [`Self::covers`] can decide
/// statically whether one pattern matches a superset of another — the property
/// [`Mounts::validate`] relies on to detect shadowed sources. A general glob
/// (`wax`) would make coverage undecidable, so mount patterns are restricted
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
    /// Parse a mount pattern. Rejects any `*` that is not one of the three
    /// recognized wildcard shapes (`**`, `@*/*`, `@<scope>/*`), and any
    /// remaining literal that is not a well-formed package name — so an
    /// unsupported glob or a typo like `@acme` (meaning `@acme/*`) fails
    /// loudly instead of being read as a literal name that silently never
    /// matches and lets the scope land on a later router source.
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
    /// (i.e. `self` ⊇ `other`). Decides source shadowing in [`Mounts::validate`].
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

/// The routing role of a mount. The per-mount serving details (upstream URL,
/// credentials, access policy, org id) live in the `config` module; this
/// captures only what routing and validation need.
///
/// A concrete mount's `patterns` are its declared namespace: the names it
/// serves and accepts publishes for, and the claim routers derive their
/// selection from. An empty list claims every name — the catch-all in any
/// router.
#[derive(Debug, Clone)]
pub enum MountKind {
    /// A pnpr-hosted registry: the authoritative origin for the packages it
    /// stores, and the only kind that accepts writes. Reads and writes are
    /// scoped to the mount's own storage namespace (its optional `org`).
    Hosted { patterns: Vec<PackagePattern> },
    /// Exactly one external origin. One URL, one credential generation, one
    /// cache namespace — not a chain and not a set of endpoints.
    Upstream { patterns: Vec<PackagePattern> },
    /// An ordered list of concrete mounts. A package resolves to the first
    /// source whose declared patterns claim it.
    Router { sources: Vec<String> },
}

impl MountKind {
    fn is_concrete(&self) -> bool {
        matches!(self, MountKind::Hosted { .. } | MountKind::Upstream { .. })
    }

    /// A concrete mount's declared namespace; `None` for a router (a router
    /// has no namespace of its own — it derives one from its sources).
    fn patterns(&self) -> Option<&[PackagePattern]> {
        match self {
            MountKind::Hosted { patterns } | MountKind::Upstream { patterns } => Some(patterns),
            MountKind::Router { .. } => None,
        }
    }
}

/// Whether a concrete mount's declared namespace claims `package`. An empty
/// pattern list claims every name.
fn namespace_claims(patterns: &[PackagePattern], package: &str) -> bool {
    patterns.is_empty() || patterns.iter().any(|pattern| pattern.matches(package))
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
    /// Resolved to exactly one concrete source mount whose declared patterns
    /// claim the package.
    Concrete { mount: &'a str, kind: ConcreteKind },
    /// No declared namespace claims this package: the addressed concrete
    /// mount's patterns don't cover it, or none of a router's sources claim
    /// it. A definitive `404` on reads and a rejection on writes, answered
    /// before storage or any upstream is consulted — never a fall-through.
    Unclaimed,
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
    /// concrete origin, enforcing every concrete mount's declared namespace at
    /// the mount itself. A concrete mount resolves to itself only when its
    /// patterns claim the package; a router resolves to the first source whose
    /// patterns claim it (authoritatively — an unclaimed package is
    /// [`Resolved::Unclaimed`], never a fall-through).
    #[must_use]
    pub fn resolve<'a>(&'a self, mount: &str, package: &str) -> Resolved<'a> {
        let Some((mount_id, kind)) = self.mounts.get_key_value(mount) else {
            return Resolved::UnknownMount;
        };
        match kind {
            MountKind::Hosted { patterns } => {
                if namespace_claims(patterns, package) {
                    Resolved::Concrete { mount: mount_id, kind: ConcreteKind::Hosted }
                } else {
                    Resolved::Unclaimed
                }
            }
            MountKind::Upstream { patterns } => {
                if namespace_claims(patterns, package) {
                    Resolved::Concrete { mount: mount_id, kind: ConcreteKind::Upstream }
                } else {
                    Resolved::Unclaimed
                }
            }
            MountKind::Router { sources } => {
                // Validation guarantees every source is a defined concrete
                // mount; a non-concrete entry here can only mean the graph was
                // built without validation, and it simply never matches.
                for source in sources {
                    match self.mounts.get_key_value(source) {
                        Some((source_id, MountKind::Hosted { patterns }))
                            if namespace_claims(patterns, package) =>
                        {
                            return Resolved::Concrete {
                                mount: source_id,
                                kind: ConcreteKind::Hosted,
                            };
                        }
                        Some((source_id, MountKind::Upstream { patterns }))
                            if namespace_claims(patterns, package) =>
                        {
                            return Resolved::Concrete {
                                mount: source_id,
                                kind: ConcreteKind::Upstream,
                            };
                        }
                        _ => {}
                    }
                }
                Resolved::Unclaimed
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
    /// could route a private name to the wrong origin or leave a source dead.
    /// Run at config load and on reload.
    pub fn validate(&self) -> Result<(), MountConfigError> {
        if let Some(target) = &self.default_target
            && !self.mounts.contains_key(target)
        {
            return Err(MountConfigError::UndefinedDefaultTarget { target: target.clone() });
        }
        for (name, kind) in &self.mounts {
            match kind {
                MountKind::Hosted { patterns } | MountKind::Upstream { patterns } => {
                    validate_namespace(name, patterns)?;
                }
                MountKind::Router { sources } => {
                    // A router with no sources can never serve any package —
                    // every request through it is a 404. That's only ever a
                    // config mistake (a hosted/upstream mount was probably
                    // intended), so reject it.
                    if sources.is_empty() {
                        return Err(MountConfigError::EmptyRouter { router: name.clone() });
                    }
                    self.validate_router(name, sources)?;
                }
            }
        }
        Ok(())
    }

    fn validate_router(&self, router: &str, sources: &[String]) -> Result<(), MountConfigError> {
        // A pattern-less source claims every name; represent that claim as an
        // explicit `**` so coverage against and by earlier sources is decided
        // by the same relation as any declared pattern.
        const CATCH_ALL: &[PackagePattern] = &[PackagePattern::All];
        let mut seen_sources: Vec<&str> = Vec::new();
        let mut seen_patterns: Vec<&PackagePattern> = Vec::new();
        for (index, source) in sources.iter().enumerate() {
            // The source must resolve to a defined concrete mount: an unknown
            // name, the router itself, or another router are all rejected, so a
            // router can only ever land on a real origin (no nesting, no cycles).
            if source == router {
                return Err(MountConfigError::SelfReferentialRouter { router: router.to_string() });
            }
            let kind = match self.mounts.get(source) {
                None => {
                    return Err(MountConfigError::UnknownSource {
                        router: router.to_string(),
                        source: source.clone(),
                    });
                }
                Some(kind) if !kind.is_concrete() => {
                    return Err(MountConfigError::NonConcreteSource {
                        router: router.to_string(),
                        source: source.clone(),
                    });
                }
                Some(kind) => kind,
            };
            if seen_sources.contains(&source.as_str()) {
                return Err(MountConfigError::DuplicateSource {
                    router: router.to_string(),
                    source: source.clone(),
                });
            }
            seen_sources.push(source);
            let patterns = match kind.patterns() {
                Some([]) | None => CATCH_ALL,
                Some(patterns) => patterns,
            };
            // A source is unreachable when every name it claims is already
            // claimed by an earlier source — the misordered-catch-all hazard
            // and its general form. Reject it so a shadowed private source is
            // a startup error, not a silent public fall-through.
            if patterns
                .iter()
                .all(|pattern| seen_patterns.iter().any(|earlier| earlier.covers(pattern)))
            {
                return Err(MountConfigError::UnreachableSource {
                    router: router.to_string(),
                    index,
                    source: source.clone(),
                });
            }
            for pattern in patterns {
                // A pattern covered by an earlier source's pattern can never be
                // selected in this router — every package it claims is already
                // routed away. The whole-source check above only fires when
                // *all* of a source's patterns are covered; catch the partial
                // case here so one dead claim of an otherwise-reachable source
                // can't silently send a private package to the origin an
                // earlier catch-all/scope claim points at. An identical claim
                // by two sources is the same defect: whichever is listed later
                // never receives the name, which is genuinely ambiguous
                // provenance the operator must resolve in the declared
                // namespaces, not by order.
                if let Some(earlier) =
                    seen_patterns.iter().find(|&&earlier| earlier.covers(pattern))
                {
                    return Err(MountConfigError::ShadowedPattern {
                        router: router.to_string(),
                        source: source.clone(),
                        pattern: pattern.to_string(),
                        by: earlier.to_string(),
                    });
                }
            }
            // Extend the seen set only after the per-pattern pass: a source's
            // own patterns may overlap each other (a mount-level redundancy,
            // not a routing defect) without shadowing anything across sources.
            seen_patterns.extend(patterns);
        }
        Ok(())
    }
}

/// Reject a duplicate pattern within one concrete mount's declared namespace.
fn validate_namespace(mount: &str, patterns: &[PackagePattern]) -> Result<(), MountConfigError> {
    for (index, pattern) in patterns.iter().enumerate() {
        if patterns[..index].contains(pattern) {
            return Err(MountConfigError::DuplicatePattern {
                mount: mount.to_string(),
                pattern: pattern.to_string(),
            });
        }
    }
    Ok(())
}

/// A static mount-configuration defect. Surfaced by [`Mounts::validate`] and by
/// [`PackagePattern::parse`]; the `config` module turns it into an
/// `InvalidConfig` so a bad mount set fails server startup and config reload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MountConfigError {
    /// An unsupported wildcard in a mount pattern.
    InvalidPattern { pattern: String },
    /// A wildcard-free mount pattern that is not a well-formed package name,
    /// so it could never match any request.
    ExactPatternNotAName { pattern: String },
    /// `defaultTarget` names a mount that does not exist.
    UndefinedDefaultTarget { target: String },
    /// A router has no sources at all, so it can never serve any package.
    EmptyRouter { router: String },
    /// A router lists itself as a source.
    SelfReferentialRouter { router: String },
    /// A router source is not a defined mount.
    UnknownSource { router: String, source: String },
    /// A router source is another router, not a concrete mount.
    NonConcreteSource { router: String, source: String },
    /// A router lists the same source more than once.
    DuplicateSource { router: String, source: String },
    /// A concrete mount declares the same pattern more than once.
    DuplicatePattern { mount: String, pattern: String },
    /// A router source's claims are fully covered by earlier sources, so it
    /// can never be selected.
    UnreachableSource { router: String, index: usize, source: String },
    /// A single pattern of a later source is covered by an earlier source's
    /// pattern, so it can never be selected in this router even though the
    /// rest of its source stays reachable.
    ShadowedPattern { router: String, source: String, pattern: String, by: String },
}

impl fmt::Display for MountConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MountConfigError::InvalidPattern { pattern } => write!(
                f,
                "unsupported mount pattern {pattern:?}: use an exact name, `@scope/*`, `@*/*`, \
                 or `**`",
            ),
            MountConfigError::ExactPatternNotAName { pattern } => write!(
                f,
                "mount pattern {pattern:?} is not a valid package name, so it can never match; \
                 to claim every package in a scope use `@scope/*`",
            ),
            MountConfigError::UndefinedDefaultTarget { target } => {
                write!(f, "defaultTarget {target:?} is not a defined mount")
            }
            MountConfigError::EmptyRouter { router } => write!(
                f,
                "router {router:?} has no sources, so it can never serve any package; add \
                 sources or remove the mount",
            ),
            MountConfigError::SelfReferentialRouter { router } => {
                write!(f, "router {router:?} lists itself as a source")
            }
            MountConfigError::UnknownSource { router, source } => {
                write!(f, "router {router:?} source {source:?} is not a defined mount")
            }
            MountConfigError::NonConcreteSource { router, source } => write!(
                f,
                "router {router:?} source {source:?} is itself a router; a source must be a \
                 hosted or upstream mount",
            ),
            MountConfigError::DuplicateSource { router, source } => {
                write!(f, "router {router:?} lists source {source:?} more than once")
            }
            MountConfigError::DuplicatePattern { mount, pattern } => {
                write!(f, "mount {mount:?} declares pattern {pattern:?} more than once")
            }
            MountConfigError::UnreachableSource { router, index, source } => write!(
                f,
                "router {router:?} source #{index} ({source:?}) is unreachable: earlier sources \
                 already claim every package it would serve; list it before the sources that \
                 shadow it, or remove it",
                index = index + 1,
            ),
            MountConfigError::ShadowedPattern { router, source, pattern, by } => write!(
                f,
                "router {router:?} can never select source {source:?} for its pattern \
                 {pattern:?}: an earlier source's pattern {by:?} already claims every package it \
                 would; reorder the sources or adjust the declared namespaces",
            ),
        }
    }
}

impl std::error::Error for MountConfigError {}

#[cfg(test)]
mod tests;
