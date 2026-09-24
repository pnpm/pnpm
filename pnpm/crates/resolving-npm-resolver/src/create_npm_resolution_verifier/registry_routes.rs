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
            && let Some(matched) = self.match_tarball_registry(url)
        {
            return matched;
        }
        pick_registry_for_package(&self.registries, &name.to_string(), None)
    }

    fn match_tarball_registry(&self, url: &str) -> Option<String> {
        // Match on the same canonical form the tarball comparison uses, so
        // a named-registry or scoped-registry tarball that differs from the
        // configured base only by scheme or `%2f` encoding still routes to its
        // registry instead of falling back (and then failing closed against the
        // wrong packument).
        // Check prefixes in descending order of specificity so a longer scoped
        // or named registry path takes precedence over a broader prefix, and
        // exclude the default registry so scoped packages fall back to scope-based
        // routing rather than validating against public metadata.
        let normalized = canonical_tarball_url(url);
        let mut candidate_prefixes: Vec<&str> = Vec::new();
        for prefix in &self.named_registry_prefixes {
            candidate_prefixes.push(prefix.as_str());
        }
        for (scope, prefix) in &self.registries {
            if scope != "default" {
                candidate_prefixes.push(prefix.as_str());
            }
        }
        candidate_prefixes.sort_by(|first, second| {
            let len_first = canonical_tarball_url(first).len();
            let len_second = canonical_tarball_url(second).len();
            len_second
                .cmp(&len_first)
                .then_with(|| first.cmp(second))
        });
        candidate_prefixes.dedup();
        for prefix in candidate_prefixes {
            if normalized.starts_with(&canonical_tarball_url(prefix)) {
                return Some(prefix.to_string());
            }
        }
        None
    }
}
