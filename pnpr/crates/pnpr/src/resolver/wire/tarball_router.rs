use super::{
    Arc, CanonicalPackageName, HashMap, Identity, Lockfile, LockfileResolution, PacquetConfig,
    RouteClass, RouteContext, TarballResolution, is_git_hosted_tarball_url, is_http_tarball_url,
    pick_registry_for_package, sanitize_registry_tarball_url, strip_url_credentials,
    tarball_basename, tarball_url_and_integrity,
};

#[derive(Clone)]
pub(in super::super) struct TarballRouter {
    pub(super) context: Arc<RouteContext>,
    pub(super) identity: Identity,
    pub(super) public_url: String,
    /// Per-scope registry map (`scope -> registry URL`, plus the default) used
    /// to classify a registry-resolved package by its *registry* route rather
    /// than its `dist.tarball` host. See [`Self::route_registry_url`].
    pub(super) registries: HashMap<String, String>,
}

impl TarballRouter {
    pub(in super::super) fn new(
        context: Arc<RouteContext>,
        identity: Identity,
        public_url: String,
        registries: HashMap<String, String>,
    ) -> Self {
        Self { context, identity, public_url, registries }
    }

    /// Route a registry-resolved package's tarball by the **registry** it came
    /// from, not its `dist.tarball` URL. A split-domain registry serves the
    /// tarball from a different host than the packument, so classifying by the
    /// tarball URL would misread a private package as public and leak its raw
    /// upstream URL. Classifying by the registry origin keeps a private
    /// package on its `/~<name>/` endpoint; a public one still emits its real
    /// (anonymously fetchable) tarball URL for a direct CDN download.
    pub(super) fn route_registry_url(
        &self,
        package: &str,
        version: &str,
        tarball_url: &str,
    ) -> String {
        let registry = pick_registry_for_package(&self.registries, package, None);
        match self.context.classify(&self.identity, &registry, Some(package)) {
            // The `dist.tarball` is untrusted upstream metadata, so sanitize it
            // before emitting/caching: drop inline `user:pass@host` userinfo and
            // any query/fragment a registry could use to carry a signed-URL
            // token. A genuinely public tarball is anonymously fetchable, so the
            // sanitized URL still works.
            RouteClass::Public => sanitize_registry_tarball_url(tarball_url),
            RouteClass::Hosted { .. } => pnpr_tarball_url(
                &self.public_url,
                &self.context.base_path(pnpr_registry::Ecosystem::Npm),
                package,
                &tarball_filename(package, version, tarball_url),
            ),
            RouteClass::Proxied { alias, .. } => upstream_endpoint_tarball_url(
                &self.public_url,
                &self.context.base_path(pnpr_registry::Ecosystem::Npm),
                &alias,
                package,
                &tarball_filename(package, version, tarball_url),
            ),
        }
    }

    pub(in super::super) fn route_lockfile(
        &self,
        config: &PacquetConfig,
        lockfile: &Lockfile,
    ) -> Lockfile {
        let mut routed = lockfile.clone();
        let Some(packages) = routed.packages.as_mut() else {
            return routed;
        };
        for (package_key, metadata) in packages {
            if !matches!(
                metadata.resolution,
                LockfileResolution::Registry(_) | LockfileResolution::Tarball(_),
            ) {
                continue;
            }
            // A resolution that pins no integrity keeps its original URL:
            // routing it through the endpoint would hand the client a
            // mirrored tarball it has no hash to check.
            let Ok((tarball_url, Some(integrity))) =
                tarball_url_and_integrity(&metadata.resolution, package_key, config)
            else {
                continue;
            };
            if !is_http_tarball_url(&tarball_url) || is_git_hosted_tarball_url(&tarball_url) {
                continue;
            }
            let name = package_key.name.to_string();
            let version = package_key.suffix.version().to_string();
            let routed_url = self.route_url(&name, &version, &tarball_url);
            if routed_url == tarball_url.as_ref() {
                continue;
            }
            metadata.resolution = LockfileResolution::Tarball(TarballResolution {
                tarball: routed_url,
                integrity: Some(integrity.clone()),
                revision: None,
                git_hosted: None,
                path: None,
            });
        }
        routed
    }

    pub(in super::super) fn verification_lockfile(&self, lockfile: &Lockfile) -> Lockfile {
        let mut upstream = lockfile.clone();
        let Some(packages) = upstream.packages.as_mut() else {
            return upstream;
        };
        for metadata in packages.values_mut() {
            let LockfileResolution::Tarball(resolution) = &mut metadata.resolution else {
                continue;
            };
            if let Some(tarball_url) = self.upstream_endpoint_tarball_url(&resolution.tarball) {
                resolution.tarball = tarball_url;
            }
        }
        upstream
    }

    pub(super) fn route_url(&self, package: &str, version: &str, tarball_url: &str) -> String {
        match self.context.classify(&self.identity, tarball_url, Some(package)) {
            // A public route keeps its upstream URL: it was fetched
            // anonymously, so its tarball is anonymously fetchable and pnpr
            // never mints a per-tarball gateway URL. Any inline userinfo a
            // malicious/compromised registry embedded in `dist.tarball` is
            // stripped first, so pnpr never streams or caches it.
            RouteClass::Public => strip_url_credentials(tarball_url),
            RouteClass::Hosted { .. } => pnpr_tarball_url(
                &self.public_url,
                &self.context.base_path(pnpr_registry::Ecosystem::Npm),
                package,
                &tarball_filename(package, version, tarball_url),
            ),
            RouteClass::Proxied { alias, .. } => upstream_endpoint_tarball_url(
                &self.public_url,
                &self.context.base_path(pnpr_registry::Ecosystem::Npm),
                &alias,
                package,
                &tarball_filename(package, version, tarball_url),
            ),
        }
    }

    /// Reverse a named endpoint tarball URL back to its
    /// upstream URL so an input lockfile carrying endpoint URLs can be verified
    /// against the real registry. Returns `None` for any other URL, and for an
    /// endpoint the caller is not authorized for (so verification cannot be
    /// used as an oracle for an upstream the caller cannot reach).
    pub(super) fn upstream_endpoint_tarball_url(&self, tarball_url: &str) -> Option<String> {
        let base_path = self.context.base_path(pnpr_registry::Ecosystem::Npm);
        let prefix = format!("{}{base_path}/~", self.public_url.trim_end_matches('/'));
        let route = tarball_url.strip_prefix(&prefix)?;
        let (upstream, rest) = route.split_once('/')?;
        let registry = self.context.upstream_registry(&self.identity, upstream)?;
        Some(format!("{}/{rest}", registry.trim_end_matches('/')))
    }
}

pub(super) fn tarball_filename(package: &str, version: &str, tarball_url: &str) -> String {
    tarball_basename(tarball_url).map_or_else(
        || {
            CanonicalPackageName::parse(package, pnpr_package_name::Ecosystem::Npm).map_or_else(
                |_| format!("{package}-{version}.tgz"),
                |name| name.tarball_name_for_version(version),
            )
        },
        str::to_string,
    )
}

pub(super) fn pnpr_tarball_url(
    public_url: &str,
    base_path: &str,
    package: &str,
    filename: &str,
) -> String {
    format!("{}{base_path}/{package}/-/{filename}", public_url.trim_end_matches('/'))
}

/// The registry-endpoint URL a proxied route's tarball is served through.
pub(super) fn upstream_endpoint_tarball_url(
    public_url: &str,
    base_path: &str,
    upstream: &str,
    package: &str,
    filename: &str,
) -> String {
    format!("{}{base_path}/~{upstream}/{package}/-/{filename}", public_url.trim_end_matches('/'))
}
