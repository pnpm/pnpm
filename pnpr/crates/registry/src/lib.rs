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

pub use pnpr_package_name::Ecosystem;

use indexmap::IndexMap;
use pnpr_package_name::CanonicalPackageName;
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
    /// `<namespace>/*` — every image repository under one leading path
    /// component. One component only, so two namespace patterns are either
    /// equal or disjoint and the specificity chain stays strict.
    Namespace(String),
    /// A literal package name (`foo`, `@scope/foo`, or `namespace/image`).
    Exact(String),
}

impl PackagePattern {
    /// Parse a registry pattern. `**` claims everything in any ecosystem; the
    /// wildcard shape below it is the one that ecosystem's names can carry,
    /// and everything else must be a well-formed name.
    ///
    /// An unsupported glob, or a typo like `@acme` meaning `@acme/*`, fails
    /// loudly rather than being read as a literal name that silently never
    /// matches and lets the scope land on a later router source.
    pub fn parse(pattern: &str, ecosystem: Ecosystem) -> Result<Self, RegistryConfigError> {
        if pattern.is_empty() {
            return Err(invalid_pattern(pattern, ecosystem));
        }
        if pattern == "**" {
            return Ok(PackagePattern::All);
        }
        match ecosystem {
            Ecosystem::Npm => Self::parse_scoped_pattern(pattern),
            Ecosystem::Oci => Self::parse_image_pattern(pattern),
            // A crate or project name is one flat token with no namespace in
            // it, so an exact name is the only claim below `**`. A scoped
            // shape here would be a claim no name of this ecosystem could
            // ever match.
            Ecosystem::Cargo | Ecosystem::Pypi => Self::parse_exact(pattern, ecosystem),
        }
    }

    /// The npm shapes: `@*/*` for every scoped name, `@<scope>/*` for one
    /// scope, or a literal name.
    fn parse_scoped_pattern(pattern: &str) -> Result<Self, RegistryConfigError> {
        if pattern == "@*/*" {
            return Ok(PackagePattern::AnyScoped);
        }
        if let Some(scope) = pattern.strip_prefix('@').and_then(|rest| rest.strip_suffix("/*")) {
            // A wildcard inside the scope is an unsupported glob; a scope
            // that request parsing would reject — `@.acme`, `@..`, a
            // separator — is a claim no valid package name can ever match.
            if scope.contains('*') {
                return Err(invalid_pattern(pattern, Ecosystem::Npm));
            }
            if !pnpr_package_name::is_safe_path_segment(scope) {
                return Err(RegistryConfigError::ScopePatternNotAScope {
                    pattern: pattern.to_string(),
                });
            }
            return Ok(PackagePattern::Scope(scope.to_string()));
        }
        Self::parse_exact(pattern, Ecosystem::Npm)
    }

    /// The image-repository shapes: `<namespace>/*` for one leading path
    /// component, or a literal repository name. An image name carries no `@`,
    /// so npm's scoped shapes are not reachable here.
    fn parse_image_pattern(pattern: &str) -> Result<Self, RegistryConfigError> {
        let Some(namespace) = pattern.strip_suffix("/*") else {
            return Self::parse_exact(pattern, Ecosystem::Oci);
        };
        // One component only, so two namespace patterns are either equal or
        // disjoint and the specificity chain below stays strict.
        if namespace.contains('*') || namespace.contains('/') {
            return Err(invalid_pattern(pattern, Ecosystem::Oci));
        }
        pnpr_package_name::canonicalize_oci_name(namespace).map(PackagePattern::Namespace).map_err(
            |_| RegistryConfigError::NamespacePatternNotANamespace { pattern: pattern.to_string() },
        )
    }

    /// A literal name, canonicalized the way a request for it will be.
    fn parse_exact(pattern: &str, ecosystem: Ecosystem) -> Result<Self, RegistryConfigError> {
        if pattern.contains('*') {
            return Err(invalid_pattern(pattern, ecosystem));
        }
        CanonicalPackageName::parse(pattern, ecosystem)
            .map(|name| PackagePattern::Exact(name.as_str().to_string()))
            .map_err(|_| RegistryConfigError::ExactPatternNotAName { pattern: pattern.to_string() })
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
            PackagePattern::Scope(_) | PackagePattern::Namespace(_) => 2,
            PackagePattern::Exact(_) => 3,
        }
    }

    /// The scope of a well-formed scoped package name (`@acme/foo` →
    /// `acme`), without its leading `@`; `None` for an unscoped or
    /// malformed name. The scope-tier key a specificity lookup consults.
    #[must_use]
    pub fn scope_of(package: &str) -> Option<&str> {
        scoped_name(package).map(|(scope, _)| scope)
    }

    /// The leading path component of a multi-component image repository name
    /// (`acme/app` → `acme`); `None` for a single-component name. The
    /// namespace-tier key a specificity lookup consults.
    #[must_use]
    pub fn namespace_of(package: &str) -> Option<&str> {
        package.split_once('/').map(|(namespace, _)| namespace)
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
            PackagePattern::Namespace(namespace) => {
                Self::namespace_of(package).is_some_and(|leading| leading == namespace)
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
        use PackagePattern::{All, AnyScoped, Exact, Namespace, Scope};
        match self {
            All => true,
            AnyScoped => match other {
                All | Namespace(_) => false,
                AnyScoped | Scope(_) => true,
                // Consistent with `matches`: an exact name is scoped only when
                // it is a well-formed `@scope/name`, never a bare `@scope`.
                Exact(name) => scoped_name(name).is_some(),
            },
            Scope(scope) => match other {
                Scope(other_scope) => other_scope == scope,
                Exact(name) => scoped_name(name).is_some_and(|(name_scope, _)| name_scope == scope),
                All | AnyScoped | Namespace(_) => false,
            },
            Namespace(namespace) => match other {
                Namespace(other_namespace) => other_namespace == namespace,
                Exact(name) => Self::namespace_of(name).is_some_and(|leading| leading == namespace),
                All | AnyScoped | Scope(_) => false,
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
            PackagePattern::Namespace(namespace) => write!(f, "{namespace}/*"),
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
    entries: IndexMap<String, Registry>,
    /// The registry the path-less base URL (`https://<pnpr>/`) aliases. `None`
    /// disables the path-less base entirely — clients must address a registry.
    default_registry: Option<String>,
    defaults: IndexMap<Ecosystem, String>,
    /// The ecosystem of every concrete registry that is not npm. A concrete
    /// registry absent here is an npm registry. A flat router can span ecosystems;
    /// a grouped router only includes sources in its own ecosystem.
    ecosystems: IndexMap<String, Ecosystem>,
}

impl Registries {
    #[must_use]
    pub fn new(registries: IndexMap<String, Registry>, default_registry: Option<String>) -> Self {
        let ecosystems = registries
            .iter()
            .filter(|(_, registry)| registry.is_concrete())
            .filter_map(|(key, _)| {
                let (prefix, _) = key.split_once('/')?;
                Ecosystem::all()
                    .find(|ecosystem| ecosystem.as_str() == prefix)
                    .map(|ecosystem| (key.clone(), ecosystem))
            })
            .collect();
        Self { entries: registries, default_registry, defaults: IndexMap::new(), ecosystems }
    }

    /// Set ecosystem-specific defaults. Each target is an internal registry key.
    #[must_use]
    pub fn with_defaults(mut self, defaults: IndexMap<Ecosystem, String>) -> Self {
        self.defaults = defaults;
        self
    }

    /// Resolve an ecosystem-local name to its serving-table key. Qualified keys
    /// are also accepted internally; HTTP callers must pass a single segment.
    #[must_use]
    pub fn addressed(&self, name: &str, ecosystem: Ecosystem) -> Option<&str> {
        let qualified = format!("{ecosystem}/{name}");
        self.entries
            .get_key_value(&qualified)
            .or_else(|| {
                self.entries.get_key_value(name).filter(|(key, kind)| match key.split_once('/') {
                    Some((prefix, _)) => prefix == ecosystem.as_str(),
                    None => !kind.is_concrete() || self.concrete_ecosystem(key) == ecosystem,
                })
            })
            .map(|(key, _)| key.as_str())
    }

    /// The name used in an ecosystem's `~name` URL, without its internal prefix.
    #[must_use]
    pub fn local_name(key: &str) -> &str {
        key.split_once('/').map_or(key, |(_, name)| name)
    }

    /// The default serving-table key for one ecosystem.
    #[must_use]
    pub fn default_for(&self, ecosystem: Ecosystem) -> Option<&str> {
        self.defaults.get(&ecosystem).map(String::as_str).or_else(|| {
            self.default_registry.as_deref().and_then(|name| self.addressed(name, ecosystem))
        })
    }

    /// Declare the ecosystem a concrete registry serves. Every registry is npm
    /// unless declared otherwise.
    #[must_use]
    pub fn with_ecosystem(mut self, registry: &str, ecosystem: Ecosystem) -> Self {
        self.ecosystems.insert(registry.to_string(), ecosystem);
        self
    }

    /// The ecosystem a concrete registry serves. `None` for a router (which
    /// serves whatever its sources do) and for an undefined registry.
    #[must_use]
    pub fn ecosystem(&self, registry: &str) -> Option<Ecosystem> {
        match self.entries.get(registry)? {
            Registry::Hosted { .. } | Registry::Upstream { .. } => {
                Some(self.concrete_ecosystem(registry))
            }
            Registry::Router { .. } => None,
        }
    }

    #[must_use]
    pub fn has_ecosystem(&self, ecosystem: Ecosystem) -> bool {
        self.entries.iter().any(|(name, registry)| {
            registry.is_concrete() && self.concrete_ecosystem(name) == ecosystem
        })
    }

    #[must_use]
    pub fn is_only_ecosystem(&self, ecosystem: Ecosystem) -> bool {
        self.has_ecosystem(ecosystem)
            && Ecosystem::all()
                .all(|candidate| candidate == ecosystem || !self.has_ecosystem(candidate))
    }

    /// The path an ecosystem's endpoints sit under: nothing when it is the
    /// only one served, `/<ecosystem>` when it shares the server.
    ///
    /// Every URL pnpr writes about itself and every route it mounts starts
    /// here, so the decision lives in one place rather than being spelled out
    /// again at each of them.
    #[must_use]
    pub fn base_path(&self, ecosystem: Ecosystem) -> String {
        if self.is_only_ecosystem(ecosystem) { String::new() } else { format!("/{ecosystem}") }
    }

    fn concrete_ecosystem(&self, registry: &str) -> Ecosystem {
        self.ecosystems.get(registry).copied().unwrap_or_default()
    }

    /// The concrete registries of `ecosystem` a request through `registry` can
    /// land on, in selection order: the registry itself when it is concrete and
    /// serves that ecosystem, a router's matching sources, nothing otherwise.
    #[must_use]
    pub fn sources(&self, registry: &str, ecosystem: Ecosystem) -> Vec<&str> {
        match self.addressed(registry, ecosystem).and_then(|key| self.entries.get_key_value(key)) {
            Some((id, Registry::Hosted { .. } | Registry::Upstream { .. })) => {
                if self.concrete_ecosystem(id) == ecosystem { vec![id] } else { Vec::new() }
            }
            Some((_, Registry::Router { sources })) => sources
                .iter()
                .filter(|source| {
                    self.entries.get(source.as_str()).is_some_and(Registry::is_concrete)
                        && self.concrete_ecosystem(source) == ecosystem
                })
                .map(String::as_str)
                .collect(),
            None => Vec::new(),
        }
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    #[must_use]
    pub fn get(&self, registry: &str) -> Option<&Registry> {
        self.entries.get(registry)
    }

    #[must_use]
    pub fn default_registry(&self) -> Option<&str> {
        self.default_registry.as_deref()
    }

    /// The internal serving-table keys, in declaration order. Grouped keys are `ecosystem/name`.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.entries.keys().map(String::as_str)
    }

    /// Whether `registry` is a defined router.
    #[must_use]
    pub fn is_router(&self, registry: &str) -> bool {
        matches!(self.entries.get(registry), Some(Registry::Router { .. }))
    }

    /// Insert `name` as a pattern-less upstream registry when it is not
    /// already declared, so the server can fold a programmatically-added
    /// upstream into the graph and keep [`Self::resolve`] the only dispatch
    /// table for `/~<name>/` traffic. An embedder that wants a namespace
    /// bound on an upstream declares its own entry (with patterns) first.
    pub fn ensure_upstream(&mut self, name: &str) {
        if !self.entries.contains_key(name) {
            self.entries.insert(name.to_string(), Registry::Upstream { patterns: Vec::new() });
            if let Some((prefix, _)) = name.split_once('/')
                && let Some(ecosystem) =
                    Ecosystem::all().find(|ecosystem| ecosystem.as_str() == prefix)
            {
                self.ecosystems.insert(name.to_string(), ecosystem);
            }
        }
    }

    /// Resolve a request addressed to `registry` for `package` in `ecosystem`
    /// to its single concrete origin, enforcing every concrete registry's
    /// declared namespace at the registry itself. A concrete registry resolves
    /// to itself only when it serves the ecosystem and its patterns claim the
    /// package; a router resolves to the first source of that ecosystem whose
    /// patterns claim it (authoritatively — an unclaimed package is
    /// [`Resolved::Unclaimed`], never a fall-through). Sources of another
    /// ecosystem are invisible to the request, so one router can front every
    /// ecosystem while each protocol's requests only ever reach origins that
    /// speak it.
    #[must_use]
    pub fn resolve<'a>(
        &'a self,
        registry: &str,
        ecosystem: Ecosystem,
        package: &str,
    ) -> Resolved<'a> {
        let Some((registry_id, kind)) =
            self.addressed(registry, ecosystem).and_then(|key| self.entries.get_key_value(key))
        else {
            return Resolved::UnknownRegistry;
        };
        let claim = |id: &'a str, kind: &Registry| -> Option<Resolved<'a>> {
            let (patterns, concrete) = match kind {
                Registry::Hosted { patterns } => (patterns, ConcreteKind::Hosted),
                Registry::Upstream { patterns } => (patterns, ConcreteKind::Upstream),
                Registry::Router { .. } => return None,
            };
            (self.concrete_ecosystem(id) == ecosystem && namespace_claims(patterns, package))
                .then_some(Resolved::Concrete { registry: id, kind: concrete })
        };
        match kind {
            Registry::Hosted { .. } | Registry::Upstream { .. } => {
                claim(registry_id, kind).unwrap_or(Resolved::Unclaimed)
            }
            // Validation guarantees every source is a defined concrete
            // registry; a non-concrete entry here can only mean the graph was
            // built without validation, and it simply never matches.
            Registry::Router { sources } => sources
                .iter()
                .filter_map(|source| self.entries.get_key_value(source))
                .find_map(|(source_id, source)| claim(source_id, source))
                .unwrap_or(Resolved::Unclaimed),
        }
    }

    /// Resolve a request to an ecosystem's path-less base (`https://<pnpr>/`,
    /// `https://<pnpr>/cargo/`, ...) through the configured default target.
    /// With no default target the path-less base is disabled, so every package
    /// is [`Resolved::UnknownRegistry`].
    #[must_use]
    pub fn resolve_default<'a>(&'a self, ecosystem: Ecosystem, package: &str) -> Resolved<'a> {
        match self.default_for(ecosystem) {
            Some(target) => self.resolve(target, ecosystem, package),
            None => Resolved::UnknownRegistry,
        }
    }

    /// Validate the whole registry set, failing closed on any configuration that
    /// could route a private name to the wrong origin or leave a source dead.
    /// Run at config load and on reload.
    pub fn validate(&self) -> Result<(), RegistryConfigError> {
        if let Some(target) = &self.default_registry
            && !self.entries.contains_key(target)
        {
            return Err(RegistryConfigError::UndefinedDefaultRegistry { target: target.clone() });
        }
        for (ecosystem, target) in &self.defaults {
            if self.addressed(target, *ecosystem).is_none() {
                return Err(RegistryConfigError::UndefinedDefaultRegistry {
                    target: target.clone(),
                });
            }
            if self.sources(target, *ecosystem).is_empty() {
                return Err(RegistryConfigError::DefaultRegistryWithoutEcosystem {
                    target: target.clone(),
                    ecosystem: *ecosystem,
                });
            }
        }
        for (name, ecosystem) in &self.ecosystems {
            match self.entries.get(name) {
                Some(kind) if kind.is_concrete() => {}
                _ => {
                    return Err(RegistryConfigError::EcosystemOnNonConcreteRegistry {
                        registry: name.clone(),
                        ecosystem: *ecosystem,
                    });
                }
            }
        }
        for (name, kind) in &self.entries {
            if let Some((prefix, local)) = name.split_once('/') {
                let ecosystem = Ecosystem::all().find(|ecosystem| ecosystem.as_str() == prefix);
                let valid = ecosystem.is_some_and(|ecosystem| {
                    let duplicate = self.entries.get(local).is_some_and(|other| {
                        !other.is_concrete() || self.concrete_ecosystem(local) == ecosystem
                    });
                    let matches = match kind {
                        Registry::Hosted { .. } | Registry::Upstream { .. } => {
                            self.concrete_ecosystem(name) == ecosystem
                        }
                        Registry::Router { sources } => sources
                            .iter()
                            .all(|source| self.concrete_ecosystem(source) == ecosystem),
                    };
                    !duplicate && matches
                });
                if !valid {
                    return Err(RegistryConfigError::InvalidRegistryIdentity {
                        registry: name.clone(),
                    });
                }
            }
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
        // Coverage is decided per ecosystem: a request only ever sees the
        // sources that speak its protocol, so a Cargo catch-all cannot shadow an
        // npm source listed after it.
        let mut seen_patterns: IndexMap<Ecosystem, Vec<&PackagePattern>> = IndexMap::new();
        for (index, source) in sources.iter().enumerate() {
            // The source must resolve to a defined concrete registry: an unknown
            // name, the router itself, or another router are all rejected, so a
            // router can only ever land on a real origin (no nesting, no cycles).
            if source == router {
                return Err(RegistryConfigError::SelfReferentialRouter {
                    router: router.to_string(),
                });
            }
            let kind = match self.entries.get(source) {
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
            let seen_patterns = seen_patterns.entry(self.concrete_ecosystem(source)).or_default();
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
    /// A qualified identity conflicts with another entry or its ecosystem.
    InvalidRegistryIdentity { registry: String },
    /// An unsupported wildcard in a registry pattern. Carries the ecosystem
    /// so the message can name the shapes that ecosystem admits, rather than
    /// sending an operator to one it always refuses.
    InvalidPattern { pattern: String, ecosystem: Ecosystem },
    /// A wildcard-free registry pattern that is not a well-formed package name,
    /// so it could never match any request.
    ExactPatternNotAName { pattern: String },
    /// A `@<scope>/*` pattern whose scope no well-formed package name can
    /// carry, so it could never match any request.
    ScopePatternNotAScope { pattern: String },
    /// A `<namespace>/*` pattern whose namespace no well-formed image
    /// repository name can carry, so it could never match any request.
    NamespacePatternNotANamespace { pattern: String },
    /// `defaultRegistry` names a registry that does not exist.
    UndefinedDefaultRegistry { target: String },
    /// An ecosystem-specific default has no concrete source serving its protocol.
    DefaultRegistryWithoutEcosystem { target: String, ecosystem: Ecosystem },
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
    /// An ecosystem is declared for a name that is not a concrete registry.
    EcosystemOnNonConcreteRegistry { registry: String, ecosystem: Ecosystem },
}

/// The wildcard shapes an ecosystem's names can carry, for the message an
/// operator reads when theirs is not one of them.
fn wildcard_shapes(ecosystem: Ecosystem) -> &'static str {
    match ecosystem {
        Ecosystem::Npm => "`@scope/*` or `@*/*`",
        Ecosystem::Oci => "`<namespace>/*`",
        // A crate or project name is one flat token, so there is no namespace
        // to claim below `**`.
        Ecosystem::Cargo | Ecosystem::Pypi => "nothing narrower",
    }
}

fn invalid_pattern(pattern: &str, ecosystem: Ecosystem) -> RegistryConfigError {
    RegistryConfigError::InvalidPattern { pattern: pattern.to_string(), ecosystem }
}

impl fmt::Display for RegistryConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RegistryConfigError::InvalidRegistryIdentity { registry } => write!(
                f,
                "registry identity {registry:?} duplicates a registry or conflicts with its ecosystem or sources",
            ),
            RegistryConfigError::InvalidPattern { pattern, ecosystem } => write!(
                f,
                "unsupported {ecosystem} registry pattern {pattern:?}: use an exact name, {}, or \
                 `**`",
                wildcard_shapes(*ecosystem),
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
            RegistryConfigError::NamespacePatternNotANamespace { pattern } => write!(
                f,
                "registry pattern {pattern:?} does not name a valid image namespace, so it can \
                 never match any repository",
            ),
            RegistryConfigError::UndefinedDefaultRegistry { target } => {
                write!(f, "defaultRegistry {target:?} is not a defined registry")
            }
            RegistryConfigError::DefaultRegistryWithoutEcosystem { target, ecosystem } => write!(
                f,
                "defaultRegistry for {ecosystem} targets {target:?}, which has no source serving {ecosystem}",
            ),
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
            RegistryConfigError::EcosystemOnNonConcreteRegistry { registry, ecosystem } => write!(
                f,
                "registry {registry:?} declares ecosystem {ecosystem} but is not a hosted or \
                 upstream registry",
            ),
        }
    }
}

impl std::error::Error for RegistryConfigError {}

#[cfg(test)]
mod tests;
