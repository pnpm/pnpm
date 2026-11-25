use super::{
    AuthHeaders,
    is_url_secure_for_credentials,
    url,
};

#[derive(Clone)]
pub(super) enum CredentialTransport {
    Registry(Vec<reqwest::Url>),
    SecureOnly,
}

impl Default for CredentialTransport {
    fn default() -> Self {
        Self::Registry(Vec::new())
    }
}

impl CredentialTransport {
    pub(super) fn requires_secure_transport(&self) -> bool {
        matches!(self, Self::SecureOnly)
    }
}

fn within_http_registry(target: &reqwest::Url, registry: &reqwest::Url) -> bool {
    let base = registry.path();
    target.origin() == registry.origin()
        && target.username().is_empty()
        && target.password().is_none()
        && path_stays_within_registry(target.path())
        && target
            .path()
            .strip_prefix(base.strip_suffix('/').unwrap_or(base))
            .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
}

fn path_stays_within_registry(path: &str) -> bool {
    if !path.contains('%') {
        return true;
    }
    let decoded = crate::percent_decode_str(path);
    !decoded.contains(['%', '\\', '\0'])
        && !decoded
            .split('/')
            .any(|segment| segment == "..")
}

impl AuthHeaders {
    /// Resolve an `Authorization` header only when `url` uses TLS or targets
    /// the local machine. The loopback exception keeps local registry proxies
    /// usable without exposing credentials on a network link.
    #[must_use]
    pub fn for_secure_url(&self, url: &str) -> Option<String> {
        self.for_secure_url_with_package(url, None)
    }

    /// Permit matching tarball credentials under HTTP registry URLs declared by the user.
    #[must_use]
    pub fn with_http_tarball_registries(mut self, registries: &[String]) -> Self {
        if self.transport.requires_secure_transport() {
            return self;
        }
        self.transport = CredentialTransport::Registry(
            registries
                .iter()
                .filter_map(|registry| {
                    let url = reqwest::Url::parse(registry).ok()?;
                    (url.scheme() == "http"
                        && url.username().is_empty()
                        && url.password().is_none()
                        && url.query().is_none()
                        && url.fragment().is_none())
                    .then_some(url)
                })
                .collect(),
        );
        self
    }

    /// Select a registry's transport policy once, before following archive redirects.
    pub fn tarball_authorization<'a>(
        &'a self,
        start: &'a str,
        package: &'a str,
    ) -> impl Fn(&str) -> Option<String> + 'a {
        let registry = self.http_tarball_registry(start, package);
        scoped_redirect_auth(start, move |url| {
            if let Some(registry) = registry {
                let target = reqwest::Url::parse(url).ok()?;
                if within_http_registry(&target, registry) {
                    return self.for_url_with_package(target.as_str(), Some(package));
                }
                return None;
            }
            self.for_secure_url_with_package(url, Some(package))
        })
    }

    fn http_tarball_registry(&self, start: &str, package: &str) -> Option<&reqwest::Url> {
        let CredentialTransport::Registry(registries) = &self.transport else { return None };
        if registries.is_empty() || self.route_hook.is_some() || package.contains("@runtime:") {
            return None;
        }
        let target = reqwest::Url::parse(start).ok()?;
        if url::secure_transport(&target) {
            return None;
        }
        registries
            .iter()
            .filter(|registry| within_http_registry(&target, registry))
            .max_by_key(|registry| registry.path().len())
    }

    /// Package-aware counterpart to [`Self::for_secure_url`].
    #[must_use]
    pub fn for_secure_url_with_package(&self, url: &str, pkg_name: Option<&str>) -> Option<String> {
        if !is_url_secure_for_credentials(url) {
            self.record_route(url, pkg_name);
            return None;
        }
        self.for_url_with_package(url, pkg_name)
    }
}

/// Keep the initial authorization on same-origin redirects within its credential scope.
pub fn scoped_redirect_auth(
    url: &str,
    lookup: impl Fn(&str) -> Option<String>,
) -> impl Fn(&str) -> Option<String> {
    let authorization = lookup(url);
    move |target| {
        let value = authorization.as_ref()?;
        if target == url {
            return Some(value.clone());
        }
        if reqwest::Url::parse(url).ok()?.origin() != reqwest::Url::parse(target).ok()?.origin() {
            return None;
        }
        lookup(target).filter(|header| header == value)
    }
}
