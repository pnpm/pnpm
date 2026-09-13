use super::{Ecosystem, IndexMap, PackagePattern, Registries, Registry, RegistryConfigError};
impl Registries {
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
            return Err(RegistryConfigError::UndefinedDefaultRegistry {
                target: target.clone(),
            });
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
        Err(RegistryConfigError::InvalidRegistryIdentity {
            registry: name.to_string(),
        })
    }

    fn identity_is_unambiguous(
        &self,
        name: &str,
        local: &str,
        kind: &Registry,
        ecosystem: Ecosystem,
    ) -> bool {
        let duplicate = self.entries
            .get(local)
            .is_some_and(|other| {
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
                    return Err(RegistryConfigError::EmptyRouter {
                        router: name.to_string(),
                    });
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
            let seen = seen_patterns
                .entry(self.concrete_ecosystem(source))
                .or_default();
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
            return Err(RegistryConfigError::SelfReferentialRouter {
                router: router.to_string(),
            });
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
    if patterns
        .iter()
        .all(|pattern| {
            seen
                .iter()
                .any(|earlier| earlier.covers(pattern))
        })
    {
        return Err(RegistryConfigError::UnreachableSource {
            router: router.to_string(),
            index,
            source: source.to_string(),
        });
    }
    for pattern in patterns {
        if let Some(earlier) = seen
            .iter()
            .find(|&&earlier| earlier.covers(pattern))
        {
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
