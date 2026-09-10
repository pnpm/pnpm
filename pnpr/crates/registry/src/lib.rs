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

pub use config_error::RegistryConfigError;

pub use package_pattern::PackagePattern;

pub use pnpr_package_name::Ecosystem;

mod config_error;

mod package_pattern;
use package_pattern::wildcard_shapes;

use indexmap::IndexMap;
use pnpr_package_name::CanonicalPackageName;
use std::fmt;

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
        self.validate_defaults()?;
        self.validate_ecosystem_targets()?;
        for (name, kind) in &self.entries {
            self.validate_entry_identity(name, kind)?;
            self.validate_entry_namespace(name, kind)?;
        }
        Ok(())
    }

    /// Every declared default must name a registry that exists and serves the
    /// ecosystem it is the default for.
    fn validate_defaults(&self) -> Result<(), RegistryConfigError> {
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
        Ok(())
    }

    /// An ecosystem can only be declared on a concrete registry: a router
    /// speaks whatever its sources speak.
    fn validate_ecosystem_targets(&self) -> Result<(), RegistryConfigError> {
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
        Ok(())
    }

    /// An `<ecosystem>/<name>` identity must name an ecosystem the registry
    /// actually serves, and must not duplicate a registry already reachable
    /// under the bare `<name>`.
    fn validate_entry_identity(
        &self,
        name: &str,
        kind: &Registry,
    ) -> Result<(), RegistryConfigError> {
        let Some((prefix, local)) = name.split_once('/') else {
            return Ok(());
        };
        let valid = Ecosystem::all()
            .find(|ecosystem| ecosystem.as_str() == prefix)
            .is_some_and(|ecosystem| self.identity_is_unambiguous(name, local, kind, ecosystem));
        if valid {
            return Ok(());
        }
        Err(RegistryConfigError::InvalidRegistryIdentity { registry: name.to_string() })
    }

    fn identity_is_unambiguous(
        &self,
        name: &str,
        local: &str,
        kind: &Registry,
        ecosystem: Ecosystem,
    ) -> bool {
        let duplicate = self.entries.get(local).is_some_and(|other| {
            !other.is_concrete() || self.concrete_ecosystem(local) == ecosystem
        });
        let matches = match kind {
            Registry::Hosted { .. } | Registry::Upstream { .. } => {
                self.concrete_ecosystem(name) == ecosystem
            }
            Registry::Router { sources } => {
                sources.iter().all(|source| self.concrete_ecosystem(source) == ecosystem)
            }
        };
        !duplicate && matches
    }

    fn validate_entry_namespace(
        &self,
        name: &str,
        kind: &Registry,
    ) -> Result<(), RegistryConfigError> {
        match kind {
            Registry::Hosted { patterns } | Registry::Upstream { patterns } => {
                validate_namespace(name, patterns)
            }
            Registry::Router { sources } => {
                // A router with no sources can never serve any package —
                // every request through it is a 404. That's only ever a
                // config mistake (a hosted/upstream registry was probably
                // intended), so reject it.
                if sources.is_empty() {
                    return Err(RegistryConfigError::EmptyRouter { router: name.to_string() });
                }
                self.validate_router(name, sources)
            }
        }
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
            let kind = self.router_source(router, source)?;
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
            let seen = seen_patterns.entry(self.concrete_ecosystem(source)).or_default();
            reject_shadowed_source(router, source, index, patterns, seen)?;
            // Extend the seen set only after the per-pattern pass: a source's
            // own patterns may overlap each other (a registry-level redundancy,
            // not a routing defect) without shadowing anything across sources.
            seen.extend(patterns);
        }
        Ok(())
    }

    /// The concrete registry a router source names. An unknown name, the
    /// router itself, or another router are all rejected, so a router can only
    /// ever land on a real origin (no nesting, no cycles).
    fn router_source(&self, router: &str, source: &str) -> Result<&Registry, RegistryConfigError> {
        if source == router {
            return Err(RegistryConfigError::SelfReferentialRouter { router: router.to_string() });
        }
        match self.entries.get(source) {
            None => Err(RegistryConfigError::UnknownSource {
                router: router.to_string(),
                source: source.to_string(),
            }),
            Some(kind) if !kind.is_concrete() => Err(RegistryConfigError::NonConcreteSource {
                router: router.to_string(),
                source: source.to_string(),
            }),
            Some(kind) => Ok(kind),
        }
    }
}

/// Reject a router source whose claims an earlier source already covers.
///
/// A source is unreachable when every name it claims is already claimed by an
/// earlier source — the misordered-catch-all hazard and its general form.
/// Rejecting it makes a shadowed private source a startup error, not a silent
/// public fall-through.
///
/// The whole-source check only fires when *all* of a source's patterns are
/// covered, so the partial case is caught per pattern: one dead claim of an
/// otherwise-reachable source would otherwise silently send a private package
/// to the origin an earlier catch-all or scope claim points at. An identical
/// claim by two sources is the same defect: whichever is listed later never
/// receives the name, which is genuinely ambiguous provenance the operator must
/// resolve in the declared namespaces, not by order.
fn reject_shadowed_source(
    router: &str,
    source: &str,
    index: usize,
    patterns: &[PackagePattern],
    seen: &[&PackagePattern],
) -> Result<(), RegistryConfigError> {
    if patterns.iter().all(|pattern| seen.iter().any(|earlier| earlier.covers(pattern))) {
        return Err(RegistryConfigError::UnreachableSource {
            router: router.to_string(),
            index,
            source: source.to_string(),
        });
    }
    for pattern in patterns {
        if let Some(earlier) = seen.iter().find(|&&earlier| earlier.covers(pattern)) {
            return Err(RegistryConfigError::ShadowedPattern {
                router: router.to_string(),
                source: source.to_string(),
                pattern: pattern.to_string(),
                by: earlier.to_string(),
            });
        }
    }
    Ok(())
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

#[cfg(test)]
mod tests;
