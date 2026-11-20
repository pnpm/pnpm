use std::collections::{
    BTreeMap,
    HashMap,
    HashSet,
};

use pnpm_lockfile::{
    PkgName,
    RegistryOptions,
};
use pnpm_resolving_resolver_base::ResolutionVerification;

use crate::{
    ReverseRoutedTarballUrl,
    create_npm_resolution_verifier::artifact_binding::canonical_tarball_url,
    named_registry::pick_registry_for_package,
    violation_codes::MISSING_NAMED_REGISTRY_VIOLATION_CODE,
};

struct PrefixCandidate {
    canonical: String,
    raw: String,
    named: bool,
    scopes: HashSet<String>,
}

/// Pre-indexed registry selector shared by [`crate::NpmResolutionVerifier`] and
/// server-side verdict-hash salting so both pick the identical registry per
/// lockfile entry without re-normalizing or re-sorting prefixes per package.
pub struct VerificationRegistryRoutes {
    pub(super) registries: HashMap<String, String>,
    sorted_prefixes: Vec<PrefixCandidate>,
    /// Alias → URL map (built-ins merged with the user's setting) for
    /// routing registry-qualified lockfile keys, which carry no tarball
    /// URL for the prefix list to match.
    pub(super) registries_by_prefix: HashMap<String, String>,
    pub(super) registry_options_by_url: Option<BTreeMap<String, RegistryOptions>>,
}

impl VerificationRegistryRoutes {
    #[must_use]
    pub fn new(
        registries: HashMap<String, String>,
        registries_by_prefix: HashMap<String, String>,
        registry_options_by_url: Option<BTreeMap<String, RegistryOptions>>,
    ) -> Self {
        let sorted_prefixes = build_sorted_prefixes(&registries, &registries_by_prefix);
        Self { registries, sorted_prefixes, registries_by_prefix, registry_options_by_url }
    }

    pub(super) fn is_pnpr_registry(&self, registry: &str) -> bool {
        self.registry_options_by_url
            .as_ref()
            .is_some_and(|options| {
                pnpm_lockfile::registry_server_type(options, registry)
                    == Some(pnpm_lockfile::RegistryServerType::Pnpr)
            })
    }

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

    pub(super) fn pick_registry(
        &self,
        name: &PkgName,
        tarball_url: Option<&str>,
        reverse: Option<&ReverseRoutedTarballUrl>,
    ) -> String {
        let reversed =
            tarball_url.and_then(|url| reverse.and_then(|rev| rev(&name.to_string(), url)));
        self.select_registry(name, None, reversed.as_deref().or(tarball_url))
            .expect("unprefixed entries always select a scope or default registry")
    }

    /// Select the single registry URL [`crate::NpmResolutionVerifier`] consults
    /// for an entry once any routed tarball URL has been reversed
    /// (`effective_tarball_url`), or `None` when `registry_name` is undeclared.
    #[must_use]
    pub fn select_registry(
        &self,
        name: &PkgName,
        registry_name: Option<&str>,
        effective_tarball_url: Option<&str>,
    ) -> Option<String> {
        if let Some(registry_name) = registry_name {
            return self.registries_by_prefix.get(registry_name).cloned();
        }
        if let Some(url) = effective_tarball_url
            && let Some(matched) = self.match_tarball_registry(url, name)
        {
            return Some(matched.to_string());
        }
        Some(pick_registry_for_package(&self.registries, &name.to_string(), None))
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
    fn match_tarball_registry(&self, url: &str, name: &PkgName) -> Option<&str> {
        let normalized = canonical_tarball_url(url);
        let own_scope = name.scope
            .as_ref()
            .map(|scope| format!("@{scope}"))
            .filter(|scope| self.registries.contains_key(scope));
        self.sorted_prefixes
            .iter()
            .filter(|candidate| {
                candidate.named
                    || match &own_scope {
                        Some(own_scope) => candidate.scopes.contains(own_scope.as_str()),
                        None => !candidate.scopes.is_empty(),
                    }
            })
            .find(|candidate| normalized.starts_with(&candidate.canonical))
            .map(|candidate| candidate.raw.as_str())
    }
}

fn build_sorted_prefixes(
    registries: &HashMap<String, String>,
    registries_by_prefix: &HashMap<String, String>,
) -> Vec<PrefixCandidate> {
    let mut by_raw: HashMap<String, (bool, HashSet<String>)> = HashMap::new();
    for prefix in crate::named_registry::named_registry_tarball_prefixes(registries_by_prefix) {
        by_raw.entry(prefix).or_default().0 = true;
    }
    for (scope, prefix) in registries {
        if scope.as_str() != "default" {
            by_raw
                .entry(prefix.clone())
                .or_default()
                .1
                .insert(scope.clone());
        }
    }
    let mut sorted: Vec<PrefixCandidate> = by_raw
        .into_iter()
        .map(|(raw, (named, scopes))| PrefixCandidate {
            canonical: canonical_tarball_url(&raw),
            raw,
            named,
            scopes,
        })
        .collect();
    sorted.sort_by(|first, second| {
        second.canonical
            .len()
            .cmp(&first.canonical.len())
            .then_with(|| first.raw.cmp(&second.raw))
    });
    sorted
}
