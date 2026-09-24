use std::collections::HashMap;

use pnpm_lockfile::PkgName;
use pnpm_resolving_resolver_base::ResolutionVerification;

use crate::{
    create_npm_resolution_verifier::artifact_binding::canonical_tarball_url,
    named_registry::pick_registry_for_package,
    violation_codes::MISSING_NAMED_REGISTRY_VIOLATION_CODE,
};

pub(super) struct VerificationRegistryRoutes {
    pub(super) registries: HashMap<String, String>,
    pub(super) named_registry_prefixes: Vec<String>,
    /// Alias → URL map (built-ins merged with the user's setting) for
    /// routing registry-qualified lockfile keys, which carry no tarball
    /// URL for the prefix list to match.
    pub(super) registries_by_prefix: HashMap<String, String>,
}

impl VerificationRegistryRoutes {
    /// The URL a registry-qualified entry routes to.
    ///
    /// Registry-qualified entries name their registry in the dep path, so
    /// routing does not depend on a recorded tarball URL (canonical URLs are
    /// omitted from the lockfile in the 12.0 format). This fails closed on
    /// an unknown alias: none of the metadata-backed checks could vouch for
    /// the entry without its registry URL.
    pub(super) fn named_registry_url(
        &self,
        registry_name: Option<&str>,
    ) -> Result<Option<String>, ResolutionVerification> {
        let Some(registry_name) = registry_name else { return Ok(None) };
        match self.registries_by_prefix.get(registry_name) {
            Some(url) => Ok(Some(url.clone())),
            None => Err(ResolutionVerification::Err {
                code: MISSING_NAMED_REGISTRY_VIOLATION_CODE,
                reason: format!(
                    "has registry prefix '{registry_name}:', which is not declared by the registries setting",
                ),
            }),
        }
    }

    pub(super) fn pick_registry(&self, name: &PkgName, tarball_url: Option<&str>) -> String {
        if let Some(url) = tarball_url
            && let Some(matched) = self.match_tarball_registry(url, name)
        {
            return matched;
        }
        pick_registry_for_package(&self.registries, &name.to_string(), None)
    }

    /// The registry whose prefix the lockfile tarball URL falls under.
    ///
    /// Prefixes are matched on the same canonical form the tarball comparison
    /// uses, so a tarball that differs from the configured base only by scheme
    /// or `%2f` encoding still routes to its registry instead of failing closed
    /// against the wrong packument. The longest prefix wins.
    ///
    /// A package whose scope has a registry of its own only matches that
    /// registry among the scope registries: the lockfile must not move
    /// `@a/pkg` off the registry `@a` is assigned to, or a registry that also
    /// proxies the public one would vouch for a same-name public package.
    /// Other packages match any scope registry. The default registry is never
    /// a candidate, since scope routing already falls back to it.
    fn match_tarball_registry(&self, url: &str, name: &PkgName) -> Option<String> {
        let normalized = canonical_tarball_url(url);
        let own_scope = name.scope
            .as_ref()
            .map(|scope| format!("@{scope}"))
            .filter(|scope| self.registries.contains_key(scope));
        let scope_prefixes = self.registries
            .iter()
            .filter(|(scope, _)| match &own_scope {
                Some(own_scope) => *scope == own_scope,
                None => scope.as_str() != "default",
            })
            .map(|(_, prefix)| prefix.as_str());
        let mut candidate_prefixes: Vec<&str> = self.named_registry_prefixes
            .iter()
            .map(String::as_str)
            .chain(scope_prefixes)
            .collect();
        candidate_prefixes.sort_by(|first, second| {
            canonical_tarball_url(second)
                .len()
                .cmp(&canonical_tarball_url(first).len())
                .then_with(|| first.cmp(second))
        });
        candidate_prefixes.dedup();
        candidate_prefixes
            .into_iter()
            .find(|prefix| normalized.starts_with(&canonical_tarball_url(prefix)))
            .map(str::to_string)
    }
}
