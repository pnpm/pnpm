//! Which upstream alias, if any, lends its credential to a fetch.

use super::{ConcreteKind, Identity, Resolved, ResolvedAlias, RouteContext};

impl RouteContext {
    /// Whether the upstream registry's credential may be used for `package`
    /// on behalf of `identity`: the upstream's `packages:` namespace claims
    /// the name, and its effective per-package access admits the caller. A
    /// non-package fetch gates at the registry level only, which alias
    /// selection already checked. An upstream with no rules entry (a
    /// programmatically folded one) skips the access check.
    pub(super) fn upstream_admits(
        &self,
        registry: &str,
        identity: &Identity,
        package: Option<&str>,
    ) -> bool {
        let Some(package) = package else {
            return true;
        };
        self.upstream_claims(registry, package)
            && self.upstream_rules
                .get(registry)
                .is_none_or(|rules| rules.access_admits(package, identity))
    }

    /// Whether `registry`'s declared namespace claims `package`, resolved in
    /// the ecosystem the upstream serves.
    pub(super) fn upstream_claims(&self, registry: &str, package: &str) -> bool {
        self.registries
            .ecosystem(registry)
            .is_some_and(|ecosystem| {
                self.registries.resolve(registry, ecosystem, package)
                    == Resolved::Concrete { registry, kind: ConcreteKind::Upstream }
            })
    }

    /// The descriptor package qualifier for a proxied fetch: `Some(package)`
    /// only when the name steers alias selection, so the cache descriptor
    /// re-checks it on replay. That is when the upstream's rules explicitly
    /// refine this name's access, or when an earlier alias on the same origin
    /// does not claim it (a package-less replay would select that alias).
    /// Other names stay registry-scoped, keeping the common footprint one
    /// descriptor per alias.
    pub(super) fn alias_package_qualifier(
        &self,
        alias: &str,
        package: Option<&str>,
    ) -> Option<String> {
        let package = package?;
        let refined = self.upstream_rules
            .get(alias)
            .is_some_and(|rules| rules.access_is_explicit(package));
        (refined || self.earlier_alias_skips(alias, package)).then(|| package.to_string())
    }

    /// Whether an alias ahead of `alias` in selection order matches its
    /// origin but does not claim `package`.
    pub(super) fn earlier_alias_skips(&self, alias: &str, package: &str) -> bool {
        let Some(selected) = self.aliases
            .iter()
            .find(|candidate| candidate.name == alias)
        else {
            return false;
        };
        self.aliases
            .iter()
            .take_while(|candidate| candidate.name != alias)
            .any(|candidate| {
                selected.origin.starts_with(&candidate.origin)
                    && !self.upstream_claims(&candidate.name, package)
            })
    }

    pub(super) fn select_alias(
        &self,
        identity: &Identity,
        fetch: &str,
        package: Option<&str>,
    ) -> Option<&ResolvedAlias> {
        self.aliases
            .iter()
            .find(|alias| {
                alias.authorization.is_some()
                    && fetch.starts_with(&alias.origin)
                    && alias.access.allows(identity)
                    && self.upstream_admits(&alias.name, identity, package)
            })
    }
}
