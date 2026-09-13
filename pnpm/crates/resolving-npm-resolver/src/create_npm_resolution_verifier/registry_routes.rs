use super::{
    MISSING_NAMED_REGISTRY_VIOLATION_CODE, PkgName, ResolutionVerification,
    VerificationRegistryRoutes, canonical_tarball_url, pick_registry_for_package,
};

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
        let Some(registry_name) = registry_name else {
            return Ok(None);
        };
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
        if let Some(url) = tarball_url {
            // Match on the same canonical form the tarball comparison uses, so
            // a named-registry tarball that differs from the configured base
            // only by scheme or `%2f` encoding still routes to its registry
            // instead of falling back (and then failing closed against the
            // wrong packument).
            let normalized = canonical_tarball_url(url);
            for prefix in &self.named_registry_prefixes {
                if normalized.starts_with(&canonical_tarball_url(prefix)) {
                    return prefix.clone();
                }
            }
        }
        pick_registry_for_package(&self.registries, &name.to_string(), None)
    }
}
