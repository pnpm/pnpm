//! Registries: the routing graph and its static validation.
//!
//! A **registry** is an addressable npm-registry surface exposed at
//! `https://<pnpr>/~<name>/`. There are two concrete kinds — a pnpr-hosted
//! registry and a single-origin upstream registry — plus one composite, a
//! **router**, an ordered list of concrete registries behind one URL.
//!
//! The model is governed by one invariant: **provenance is declared, never
//! inferred.** Every concrete registry declares the package-name patterns it
//! serves — its namespace — and that namespace is enforced on the registry
//! itself, on every path to it: an unclaimed name is a definitive not-found
//! before storage or the upstream is consulted, whether the request came
//! through a router or addressed the registry directly. A router selects the
//! first listed source whose patterns claim the name — authoritatively. It can
//! order competing claims, but it can never assign a name to a registry that does
//! not claim it, so no configuration can express a cross-origin fall-through:
//! a selected source's "not found" or "unavailable" is final. There is no
//! existence-based fallback, no mirror group, and no multi-endpoint failover.
//!
//! Because selection is first-source-in-order, source order is load-bearing: a
//! misordered router is the one way a configuration mistake could silently send
//! a private scope to a public origin. [`Registries::validate`] rejects that class
//! at config load (and reload) — shadowed/unreachable sources, duplicate
//! sources and patterns, and sources that are unknown, self-referential, or
//! not concrete. The check is static because [`PackagePattern`]'s coverage
//! relation is decidable for this deliberately small glob language.

use crate::package_name::PackageName;
use indexmap::IndexMap;
use std::fmt;

/// A package-name pattern: one member of a concrete registry's declared
/// namespace.
///
/// Deliberately a small, **decidable** language so [`Self::covers`] can decide
/// statically whether one pattern matches a superset of another — the property
/// [`Registries::validate`] relies on to detect shadowed sources. A general glob
/// (`wax`) would make coverage undecidable, so registry patterns are restricted
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
    /// Parse a registry pattern. Rejects any `*` that is not one of the three
    /// recognized wildcard shapes (`**`, `@*/*`, `@<scope>/*`), and any
    /// remaining literal that is not a well-formed package name — so an
    /// unsupported glob or a typo like `@acme` (meaning `@acme/*`) fails
    /// loudly instead of being read as a literal name that silently never
    /// matches and lets the scope land on a later router source.
    pub fn parse(pattern: &str) -> Result<Self, RegistryConfigError> {
        let invalid = || RegistryConfigError::InvalidPattern { pattern: pattern.to_string() };
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
            // A single, concrete scope: `@acme/*`. A wildcard inside the
            // scope is an unsupported glob; a scope that request parsing
            // (`PackageName::parse`) would reject — `@.acme`, `@..`, a
            // separator — is a claim no valid package name can ever match,
            // so both fail loudly instead of becoming a dead pattern that
            // silently lets the scope land on a later router source.
            if scope.contains('*') {
                return Err(invalid());
            }
            if !crate::package_name::is_safe_path_segment(scope) {
                return Err(RegistryConfigError::ScopePatternNotAScope {
                    pattern: pattern.to_string(),
                });
            }
            return Ok(PackagePattern::Scope(scope.to_string()));
        }
        // Anything left that still carries a `*` is an unsupported glob.
        if pattern.contains('*') {
            return Err(invalid());
        }
        if PackageName::parse(pattern).is_err() {
            return Err(RegistryConfigError::ExactPatternNotAName { pattern: pattern.to_string() });
        }
        Ok(PackagePattern::Exact(pattern.to_string()))
    }

    /// How specific this pattern is: an exact name beats `@scope/*` beats
    /// `@*/*` beats `**`. For any one package name, the patterns that can
    /// match it form a strict chain — at most one per tier can exist in a
    /// duplicate-free set — so most-specific-match selection is total and
    /// order-free. That is what lets a registry's `packages:` map be a YAML
    /// mapping whose key order carries no meaning.
    #[must_use]
    pub fn specificity(&self) -> u8 {
        match self {
            PackagePattern::All => 0,
            PackagePattern::AnyScoped => 1,
            PackagePattern::Scope(_) => 2,
            PackagePattern::Exact(_) => 3,
        }
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
    /// (i.e. `self` ⊇ `other`). Decides source shadowing in [`Registries::validate`].
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

/// The routing role of a registry. The per-registry serving details (upstream URL,
/// credentials, access policy, org id) live in the `config` module; this
/// captures only what routing and validation need.
///
/// A concrete registry's `patterns` are its declared namespace: the names it
/// serves and accepts publishes for, and the claim routers derive their
/// selection from. An empty list claims every name — the catch-all in any
/// router.
#[derive(Debug, Clone)]
pub enum Registry {
    /// A pnpr-hosted registry: the authoritative origin for the packages it
    /// stores, and the only kind that accepts writes. Reads and writes are
    /// scoped to the registry's own storage namespace (its optional `org`).
    Hosted { patterns: Vec<PackagePattern> },
    /// Exactly one external origin. One URL, one credential generation, one
    /// cache namespace — not a chain and not a set of endpoints.
    Upstream { patterns: Vec<PackagePattern> },
    /// An ordered list of concrete registries. A package resolves to the first
    /// source whose declared patterns claim it.
    Router { sources: Vec<String> },
}

impl Registry {
    fn is_concrete(&self) -> bool {
        matches!(self, Registry::Hosted { .. } | Registry::Upstream { .. })
    }

    /// A concrete registry's declared namespace; `None` for a router (a router
    /// has no namespace of its own — it derives one from its sources).
    fn patterns(&self) -> Option<&[PackagePattern]> {
        match self {
            Registry::Hosted { patterns } | Registry::Upstream { patterns } => Some(patterns),
            Registry::Router { .. } => None,
        }
    }
}

/// Whether a concrete registry's declared namespace claims `package`. An empty
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

/// The outcome of resolving a request `(registry, package)` to a concrete origin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolved<'a> {
    /// Resolved to exactly one concrete source registry whose declared patterns
    /// claim the package.
    Concrete { registry: &'a str, kind: ConcreteKind },
    /// No declared namespace claims this package: the addressed concrete
    /// registry's patterns don't cover it, or none of a router's sources claim
    /// it. A definitive `404` on reads and a rejection on writes, answered
    /// before storage or any upstream is consulted — never a fall-through.
    Unclaimed,
    /// The addressed registry id is not defined.
    UnknownRegistry,
}

/// The validated set of registries plus the optional path-less default
/// target. Built and validated by the `config` module at load time.
#[derive(Debug, Default, Clone)]
pub struct Registries {
    registries: IndexMap<String, Registry>,
    /// The registry the path-less base URL (`https://<pnpr>/`) aliases. `None`
    /// disables the path-less base entirely — clients must address a registry.
    default_registry: Option<String>,
}

impl Registries {
    #[must_use]
    pub fn new(registries: IndexMap<String, Registry>, default_registry: Option<String>) -> Self {
        Self { registries, default_registry }
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.registries.is_empty()
    }

    #[must_use]
    pub fn get(&self, registry: &str) -> Option<&Registry> {
        self.registries.get(registry)
    }

    #[must_use]
    pub fn default_registry(&self) -> Option<&str> {
        self.default_registry.as_deref()
    }

    /// The declared registry names, in declaration order.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.registries.keys().map(String::as_str)
    }

    /// Whether `registry` is a defined router.
    #[must_use]
    pub fn is_router(&self, registry: &str) -> bool {
        matches!(self.registries.get(registry), Some(Registry::Router { .. }))
    }

    /// Insert `name` as a pattern-less upstream registry when it is not
    /// already declared, so the server can fold a programmatically-added
    /// upstream into the graph and keep [`Self::resolve`] the only dispatch
    /// table for `/~<name>/` traffic. An embedder that wants a namespace
    /// bound on an upstream declares its own entry (with patterns) first.
    pub fn ensure_upstream(&mut self, name: &str) {
        if !self.registries.contains_key(name) {
            self.registries.insert(name.to_string(), Registry::Upstream { patterns: Vec::new() });
        }
    }

    /// Resolve a request addressed to `registry` for `package` to its single
    /// concrete origin, enforcing every concrete registry's declared namespace at
    /// the registry itself. A concrete registry resolves to itself only when its
    /// patterns claim the package; a router resolves to the first source whose
    /// patterns claim it (authoritatively — an unclaimed package is
    /// [`Resolved::Unclaimed`], never a fall-through).
    #[must_use]
    pub fn resolve<'a>(&'a self, registry: &str, package: &str) -> Resolved<'a> {
        let Some((registry_id, kind)) = self.registries.get_key_value(registry) else {
            return Resolved::UnknownRegistry;
        };
        match kind {
            Registry::Hosted { patterns } => {
                if namespace_claims(patterns, package) {
                    Resolved::Concrete { registry: registry_id, kind: ConcreteKind::Hosted }
                } else {
                    Resolved::Unclaimed
                }
            }
            Registry::Upstream { patterns } => {
                if namespace_claims(patterns, package) {
                    Resolved::Concrete { registry: registry_id, kind: ConcreteKind::Upstream }
                } else {
                    Resolved::Unclaimed
                }
            }
            Registry::Router { sources } => {
                // Validation guarantees every source is a defined concrete
                // registry; a non-concrete entry here can only mean the graph was
                // built without validation, and it simply never matches.
                for source in sources {
                    match self.registries.get_key_value(source) {
                        Some((source_id, Registry::Hosted { patterns }))
                            if namespace_claims(patterns, package) =>
                        {
                            return Resolved::Concrete {
                                registry: source_id,
                                kind: ConcreteKind::Hosted,
                            };
                        }
                        Some((source_id, Registry::Upstream { patterns }))
                            if namespace_claims(patterns, package) =>
                        {
                            return Resolved::Concrete {
                                registry: source_id,
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
    /// disabled, so every package is [`Resolved::UnknownRegistry`].
    #[must_use]
    pub fn resolve_default<'a>(&'a self, package: &str) -> Resolved<'a> {
        match self.default_registry.as_deref() {
            Some(target) => self.resolve(target, package),
            None => Resolved::UnknownRegistry,
        }
    }

    /// Validate the whole registry set, failing closed on any configuration that
    /// could route a private name to the wrong origin or leave a source dead.
    /// Run at config load and on reload.
    pub fn validate(&self) -> Result<(), RegistryConfigError> {
        if let Some(target) = &self.default_registry
            && !self.registries.contains_key(target)
        {
            return Err(RegistryConfigError::UndefinedDefaultRegistry { target: target.clone() });
        }
        for (name, kind) in &self.registries {
            match kind {
                Registry::Hosted { patterns } | Registry::Upstream { patterns } => {
                    validate_namespace(name, patterns)?;
                }
                Registry::Router { sources } => {
                    // A router with no sources can never serve any package —
                    // every request through it is a 404. That's only ever a
                    // config mistake (a hosted/upstream registry was probably
                    // intended), so reject it.
                    if sources.is_empty() {
                        return Err(RegistryConfigError::EmptyRouter { router: name.clone() });
                    }
                    self.validate_router(name, sources)?;
                }
            }
        }
        Ok(())
    }

    fn validate_router(&self, router: &str, sources: &[String]) -> Result<(), RegistryConfigError> {
        // A pattern-less source claims every name; represent that claim as an
        // explicit `**` so coverage against and by earlier sources is decided
        // by the same relation as any declared pattern.
        const CATCH_ALL: &[PackagePattern] = &[PackagePattern::All];
        let mut seen_sources: Vec<&str> = Vec::new();
        let mut seen_patterns: Vec<&PackagePattern> = Vec::new();
        for (index, source) in sources.iter().enumerate() {
            // The source must resolve to a defined concrete registry: an unknown
            // name, the router itself, or another router are all rejected, so a
            // router can only ever land on a real origin (no nesting, no cycles).
            if source == router {
                return Err(RegistryConfigError::SelfReferentialRouter {
                    router: router.to_string(),
                });
            }
            let kind = match self.registries.get(source) {
                None => {
                    return Err(RegistryConfigError::UnknownSource {
                        router: router.to_string(),
                        source: source.clone(),
                    });
                }
                Some(kind) if !kind.is_concrete() => {
                    return Err(RegistryConfigError::NonConcreteSource {
                        router: router.to_string(),
                        source: source.clone(),
                    });
                }
                Some(kind) => kind,
            };
            if seen_sources.contains(&source.as_str()) {
                return Err(RegistryConfigError::DuplicateSource {
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
                return Err(RegistryConfigError::UnreachableSource {
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
                    return Err(RegistryConfigError::ShadowedPattern {
                        router: router.to_string(),
                        source: source.clone(),
                        pattern: pattern.to_string(),
                        by: earlier.to_string(),
                    });
                }
            }
            // Extend the seen set only after the per-pattern pass: a source's
            // own patterns may overlap each other (a registry-level redundancy,
            // not a routing defect) without shadowing anything across sources.
            seen_patterns.extend(patterns);
        }
        Ok(())
    }
}

/// Reject a duplicate pattern within one concrete registry's declared namespace.
fn validate_namespace(
    registry: &str,
    patterns: &[PackagePattern],
) -> Result<(), RegistryConfigError> {
    for (index, pattern) in patterns.iter().enumerate() {
        if patterns[..index].contains(pattern) {
            return Err(RegistryConfigError::DuplicatePattern {
                registry: registry.to_string(),
                pattern: pattern.to_string(),
            });
        }
    }
    Ok(())
}

/// A static registry-configuration defect. Surfaced by [`Registries::validate`] and by
/// [`PackagePattern::parse`]; the `config` module turns it into an
/// `InvalidConfig` so a bad registry set fails server startup and config reload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegistryConfigError {
    /// An unsupported wildcard in a registry pattern.
    InvalidPattern { pattern: String },
    /// A wildcard-free registry pattern that is not a well-formed package name,
    /// so it could never match any request.
    ExactPatternNotAName { pattern: String },
    /// A `@<scope>/*` pattern whose scope no well-formed package name can
    /// carry, so it could never match any request.
    ScopePatternNotAScope { pattern: String },
    /// `defaultRegistry` names a registry that does not exist.
    UndefinedDefaultRegistry { target: String },
    /// A router has no sources at all, so it can never serve any package.
    EmptyRouter { router: String },
    /// A router lists itself as a source.
    SelfReferentialRouter { router: String },
    /// A router source is not a defined registry.
    UnknownSource { router: String, source: String },
    /// A router source is another router, not a concrete registry.
    NonConcreteSource { router: String, source: String },
    /// A router lists the same source more than once.
    DuplicateSource { router: String, source: String },
    /// A concrete registry declares the same pattern more than once.
    DuplicatePattern { registry: String, pattern: String },
    /// A router source's claims are fully covered by earlier sources, so it
    /// can never be selected.
    UnreachableSource { router: String, index: usize, source: String },
    /// A single pattern of a later source is covered by an earlier source's
    /// pattern, so it can never be selected in this router even though the
    /// rest of its source stays reachable.
    ShadowedPattern { router: String, source: String, pattern: String, by: String },
}

impl fmt::Display for RegistryConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RegistryConfigError::InvalidPattern { pattern } => write!(
                f,
                "unsupported registry pattern {pattern:?}: use an exact name, `@scope/*`, `@*/*`, \
                 or `**`",
            ),
            RegistryConfigError::ExactPatternNotAName { pattern } => write!(
                f,
                "registry pattern {pattern:?} is not a valid package name, so it can never match; \
                 to claim every package in a scope use `@scope/*`",
            ),
            RegistryConfigError::ScopePatternNotAScope { pattern } => write!(
                f,
                "registry pattern {pattern:?} does not name a valid scope, so it can never match \
                 any package",
            ),
            RegistryConfigError::UndefinedDefaultRegistry { target } => {
                write!(f, "defaultRegistry {target:?} is not a defined registry")
            }
            RegistryConfigError::EmptyRouter { router } => write!(
                f,
                "router {router:?} has no sources, so it can never serve any package; add \
                 sources or remove the registry",
            ),
            RegistryConfigError::SelfReferentialRouter { router } => {
                write!(f, "router {router:?} lists itself as a source")
            }
            RegistryConfigError::UnknownSource { router, source } => {
                write!(f, "router {router:?} source {source:?} is not a defined registry")
            }
            RegistryConfigError::NonConcreteSource { router, source } => write!(
                f,
                "router {router:?} source {source:?} is itself a router; a source must be a \
                 hosted or upstream registry",
            ),
            RegistryConfigError::DuplicateSource { router, source } => {
                write!(f, "router {router:?} lists source {source:?} more than once")
            }
            RegistryConfigError::DuplicatePattern { registry, pattern } => {
                write!(f, "registry {registry:?} declares pattern {pattern:?} more than once")
            }
            RegistryConfigError::UnreachableSource { router, index, source } => write!(
                f,
                "router {router:?} source #{index} ({source:?}) is unreachable: earlier sources \
                 already claim every package it would serve; list it before the sources that \
                 shadow it, or remove it",
                index = index + 1,
            ),
            RegistryConfigError::ShadowedPattern { router, source, pattern, by } => write!(
                f,
                "router {router:?} can never select source {source:?} for its pattern \
                 {pattern:?}: an earlier source's pattern {by:?} already claims every package it \
                 would; reorder the sources or adjust the declared namespaces",
            ),
        }
    }
}

impl std::error::Error for RegistryConfigError {}

#[cfg(test)]
mod tests;
