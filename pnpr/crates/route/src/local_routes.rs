use super::{
    ConcreteKind,
    Ecosystem,
    Identity,
    Resolved,
    RouteContext,
    addressed_registry_segment,
    nerf_dart,
};

impl RouteContext {
    /// Whether a fetch addresses this server's configured public URL.
    #[must_use]
    pub(super) fn is_own_url(&self, url: &str) -> bool {
        self.hosted_origin
            .as_ref()
            .is_some_and(|origin| nerf_dart(url).starts_with(origin))
    }

    pub(super) fn resolve_own_origin<'a>(
        &'a self,
        fetch: &str,
        hosted: &str,
        package: &str,
    ) -> Resolved<'a> {
        let base = hosted.trim_end_matches('/');
        for ecosystem in Ecosystem::all() {
            let endpoint = format!("{base}{}/", ecosystem.base_path());
            if let Some(registry) = addressed_registry_segment(fetch, &endpoint) {
                let decoded = pnpm_network::percent_decode_str(registry);
                return self.registries.resolve(&decoded, ecosystem, package);
            }
            if fetch.starts_with(&endpoint) {
                return self.registries.resolve_default(ecosystem, package);
            }
        }
        self.registries.resolve_default(Ecosystem::Npm, package)
    }

    /// Whether `url` targets a package on this server's own origin that
    /// `identity` is not permitted to read, so `metadata_scope` must not
    /// consult or fall back to the shared public mirror.
    pub(super) fn is_denied_own_package(
        &self,
        identity: &Identity,
        url: &str,
        package: Option<&str>,
    ) -> bool {
        let Some(package) = package else {
            return false;
        };
        let fetch = nerf_dart(url);
        let Some(hosted) = self.hosted_origin.as_deref() else {
            return false;
        };
        if !fetch.starts_with(hosted) || self.is_public_route(&fetch, Some(package)) {
            return false;
        }
        match self.resolve_own_origin(&fetch, hosted, package) {
            Resolved::Concrete {
                registry,
                kind: ConcreteKind::Hosted,
            } => self.hosted_rules
                .get(registry)
                .is_some_and(|rules| !rules.access_admits(package, identity)),
            Resolved::Concrete {
                registry,
                kind: ConcreteKind::Upstream,
            } => {
                let has_alias = self.aliases
                    .iter()
                    .any(|alias| alias.name == registry);
                has_alias
                    && self
                        .authorized_alias(identity, registry)
                        .is_none_or(|alias| {
                            !self.upstream_admits(&alias.name, identity, Some(package))
                        })
            }
            Resolved::Unclaimed | Resolved::UnknownRegistry => true,
        }
    }

    pub(super) fn local_package(&self, url: &str) -> Option<String> {
        let url = url.split(['?', '#']).next()?;
        let fetch = nerf_dart(&format!("{url}/"));
        let tail = fetch
            .strip_prefix(self.hosted_origin.as_deref()?)?
            .strip_prefix("npm/")?;
        let decoded = pnpm_network::percent_decode_str(tail);
        let mut segments = decoded.split('/');
        let mut first = segments.next()?;
        if first.starts_with('~') {
            first = segments.next()?;
        }
        let identifier = if first.starts_with('@') {
            format!("{first}/{}", segments.next()?)
        } else if !first.is_empty() && first != "-" {
            first.to_string()
        } else {
            return None;
        };
        Some(
            split_package_identifier(&identifier)
                .map(|(name, _)| name.to_string())
                .unwrap_or_else(|| identifier.clone()),
        )
    }
}

/// Split `<name>@<version>` into its parts. A package name holds `@` only as
/// its scope prefix, so the version follows the last `@` past the first byte.
#[must_use]
pub fn split_package_identifier(identifier: &str) -> Option<(&str, &str)> {
    let at = identifier
        .rfind('@')
        .filter(|at| *at > 0)?;
    let (name, version) = (&identifier[..at], &identifier[at + 1..]);
    (!version.is_empty() && !name.ends_with('/')).then_some((name, version))
}

#[cfg(test)]
mod tests;
