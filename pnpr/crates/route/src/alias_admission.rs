//! Which upstream alias, if any, lends its credential to a fetch.

use super::{ConcreteKind, Identity, Resolved, ResolvedAlias, RouteContext};

impl RouteContext {
    /// Whether the upstream registry's credential may be used for `package`
    /// on behalf of `identity`: the upstream's `packages:` namespace claims
    /// the name, and a grant lets the caller read it. A
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

    /// Reuse the cached resolution only if `identity` would *select*
    /// this exact alias for its origin — the first authorized alias
    /// [`Self::select_alias`] returns there — and its credential
    /// still hashes to the same digest. Not merely an alias the
    /// caller is authorized for: with overlapping upstream access
    /// (several aliases on one origin a caller can use), an
    /// authorization-only check could replay a lockfile routed
    /// through a different `/~<name>/` endpoint than this caller
    /// resolves through. A since-removed alias (`find` → `None`) or
    /// a rotated credential also fails closed here.
    pub(super) fn allows_alias(
        &self,
        identity: &Identity,
        (alias, credential_digest): (&str, &str),
        package: Option<&str>,
    ) -> bool {
        self.aliases
            .iter()
            .find(|candidate| candidate.name == alias)
            .is_some_and(|candidate| {
                if candidate.outgrew(package) {
                    return false;
                }
                if candidate.authorization.is_none() {
                    return candidate.admits(identity)
                        && candidate.credential_digest == credential_digest
                        && self.upstream_admits(alias, identity, package);
                }
                self.select_alias(identity, &candidate.origin, package)
                    .is_some_and(|selected| {
                        selected.name == alias && selected.credential_digest == credential_digest
                    })
            })
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
    /// re-checks it on replay. That is when who may read differs between the
    /// upstream's packages, or when an earlier alias on the same origin does
    /// not claim it (a package-less replay would select that alias). Other
    /// names stay registry-scoped, keeping the common footprint one
    /// descriptor per alias.
    pub(super) fn alias_package_qualifier(
        &self,
        alias: &str,
        package: Option<&str>,
    ) -> Option<String> {
        let package = package?;
        let refined =
            self.upstream_rules.get(alias).is_some_and(pnpr_policy::PackageRules::refines_access);
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
                    && alias.admits(identity)
                    && self.upstream_admits(&alias.name, identity, package)
            })
    }
}
