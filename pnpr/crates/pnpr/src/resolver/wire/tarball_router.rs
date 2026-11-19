use super::{
    Arc, BTreeMap, BUILTIN_REGISTRIES_BY_PREFIX, CanonicalPackageName, HashMap, Identity, Lockfile,
    LockfileResolution, MappedTarballUrl, PackageKey, PackageMetadata, PacquetConfig,
    PublishedTarballUrlMapper, ReverseRoutedTarballUrl, RouteClass, RouteContext,
    TarballResolution, is_git_hosted_tarball_url, is_http_tarball_url, pick_registry_for_package,
    sanitize_registry_tarball_url, strip_url_credentials, tarball_basename,
    tarball_basename_for_package, tarball_url_and_integrity, version_route_tarball_version,
};

mod verdict_salt;

#[derive(Clone)]
pub(in super::super) struct TarballRouter {
    pub(super) context: Arc<RouteContext>,
    pub(super) identity: Identity,
    pub(super) public_url: String,
    /// Per-scope registry map (`scope -> registry URL`, plus the default) used
    /// to classify a registry-resolved package by its *registry* route rather
    /// than its `dist.tarball` host. See [`Self::route_registry_hint`].
    pub(super) registries: HashMap<String, String>,
    /// Named-registry prefix map (`alias -> registry URL`) for `<alias>:<version>`
    /// resolutions.
    pub(super) registries_by_prefix: BTreeMap<String, String>,
}

impl TarballRouter {
    pub(in super::super) fn new(
        context: Arc<RouteContext>,
        identity: Identity,
        public_url: String,
        registries: HashMap<String, String>,
    ) -> Self {
        Self { context, identity, public_url, registries, registries_by_prefix: BTreeMap::new() }
    }

    pub(in super::super) fn with_registries_by_prefix(
        mut self,
        registries_by_prefix: BTreeMap<String, String>,
    ) -> Self {
        self.registries_by_prefix = registries_by_prefix;
        self
    }

    /// Route a registry-resolved package's tarball by the **registry** it came
    /// from, not its `dist.tarball` URL. A split-domain registry serves the
    /// tarball from a different host than the packument, so classifying by the
    /// tarball URL would misread a private package as public and leak its raw
    /// upstream URL. Classifying by the registry origin keeps a private
    /// package on its `/~<name>/` endpoint; a public one still emits its real
    /// (anonymously fetchable) tarball URL for a direct CDN download.
    pub(super) fn route_registry_hint(
        &self,
        package: &str,
        version: &str,
        id: &str,
        tarball_url: &str,
    ) -> String {
        if let Some(prefix) = hint_registry_prefix(id, package) {
            let Some(registry) = self.named_registry_url(None, prefix) else {
                return sanitize_registry_tarball_url(tarball_url);
            };
            return self.route_for_registry(registry, package, version, tarball_url);
        }
        let registry = pick_registry_for_package(&self.registries, package, None);
        self.route_for_registry(&registry, package, version, tarball_url)
    }

    fn route_for_registry(
        &self,
        registry: &str,
        package: &str,
        version: &str,
        tarball_url: &str,
    ) -> String {
        match self.context.classify(&self.identity, registry, Some(package)) {
            // The `dist.tarball` is untrusted upstream metadata, so sanitize it
            // before emitting/caching: drop inline `user:pass@host` userinfo and
            // any query/fragment a registry could use to carry a signed-URL
            // token. A genuinely public tarball is anonymously fetchable, so the
            // sanitized URL still works.
            RouteClass::Public => sanitize_registry_tarball_url(tarball_url),
            RouteClass::Hosted { .. } => {
                pnpr_tarball_url(&self.public_url, package, version, tarball_url)
            }
            RouteClass::Proxied { alias, .. } => upstream_endpoint_tarball_url(
                &self.public_url,
                &alias,
                package,
                version,
                tarball_url,
            ),
        }
    }

    /// Route one lockfile entry's tarball URL: registry-resolved packages are
    /// classified by their registry origin (preserving split-domain routing and
    /// stripping presigned query tokens), while direct URL tarball dependencies
    /// are classified by their own URL.
    pub(super) fn route_lockfile_tarball(
        &self,
        config: &PacquetConfig,
        package_key: &PackageKey,
        resolution: &LockfileResolution,
        tarball_url: &str,
    ) -> String {
        let name = package_key.name.to_string();
        if !is_registry_package_resolution(package_key, resolution) {
            let version = package_key.suffix.version().to_string();
            return self.route_url(&name, &version, tarball_url);
        }
        let version = package_key.suffix
            .registry_version()
            .expect("checked in is_registry_package_resolution")
            .to_string();
        if let Some((registry_name, _)) = package_key.suffix.registry_qualified() {
            let Some(registry) = self.named_registry_url(Some(config), registry_name) else {
                return sanitize_registry_tarball_url(tarball_url);
            };
            return self.route_for_registry(registry, &name, &version, tarball_url);
        }
        let registry = self.scope_registry_for_package(config, &name);
        self.route_for_registry(&registry, &name, &version, tarball_url)
    }

    fn scope_registry_for_package(&self, config: &PacquetConfig, package: &str) -> String {
        if !self.registries.is_empty() {
            return pick_registry_for_package(&self.registries, package, None);
        }
        let registries: HashMap<String, String> = config
            .resolved_registries()
            .into_iter()
            .collect();
        pick_registry_for_package(&registries, package, None)
    }

    fn named_registry_url<'a>(
        &'a self,
        config: Option<&'a PacquetConfig>,
        registry_name: &str,
    ) -> Option<&'a str> {
        config
            .and_then(|cfg| cfg.registries_by_prefix.get(registry_name).map(String::as_str))
            .or_else(|| self.registries_by_prefix.get(registry_name).map(String::as_str))
            .or_else(|| {
                BUILTIN_REGISTRIES_BY_PREFIX
                    .iter()
                    .find_map(|(name, url)| (*name == registry_name).then_some(*url))
            })
    }

    /// Mapper for [`pnpm_resolving_npm_resolver::VerificationArtifacts`]:
    /// public routes accept both the published URL and its sanitized form,
    /// while hosted and proxied routes resolve exclusively through their
    /// authorized `pnpr` endpoint without exposing raw CDN URLs or query tokens.
    pub(in super::super) fn published_tarball_mapper(&self) -> PublishedTarballUrlMapper {
        let router = self.clone();
        Arc::new(move |registry, package, version, lockfile_tarball, published_tarball| {
            router.expected_verification_tarball(
                registry,
                package,
                version,
                lockfile_tarball,
                published_tarball,
            )
        })
    }

    pub(in super::super) fn reverse_routed_tarball_mapper(&self) -> ReverseRoutedTarballUrl {
        let router = self.clone();
        Arc::new(move |package, tarball_url| {
            router.upstream_endpoint_tarball_url(tarball_url, Some(package))
        })
    }

    fn expected_verification_tarball(
        &self,
        registry: &str,
        package: &str,
        version: &str,
        lockfile_tarball: &str,
        published_tarball: &str,
    ) -> MappedTarballUrl {
        match self.context.classify(&self.identity, registry, Some(package)) {
            RouteClass::Public => {
                let sanitized = sanitize_registry_tarball_url(published_tarball);
                MappedTarballUrl {
                    accepted_routed_url: (sanitized != published_tarball).then(|| {
                        sanitized.clone()
                    }),
                    display_url: sanitized,
                    accept_published_url: true,
                }
            }
            RouteClass::Hosted { .. } => {
                let hosted =
                    pnpr_tarball_url(&self.public_url, package, version, published_tarball);
                MappedTarballUrl {
                    accepted_routed_url: Some(hosted.clone()),
                    display_url: hosted,
                    accept_published_url: false,
                }
            }
            RouteClass::Proxied { alias, .. } => {
                let display_url = upstream_endpoint_tarball_url(
                    &self.public_url,
                    &alias,
                    package,
                    version,
                    published_tarball,
                );
                let accepted_routed_url = self
                    .upstream_endpoint_tarball_url(lockfile_tarball, Some(package))
                    .is_some()
                    .then(|| self.context.upstream_registry(&self.identity, &alias, Some(package)))
                    .flatten()
                    .map(|_| display_url.clone());
                MappedTarballUrl { display_url, accepted_routed_url, accept_published_url: false }
            }
        }
    }

    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "unconditional wrapper retained for unit tests")
    )]
    pub(in super::super) fn route_lockfile(
        &self,
        config: &PacquetConfig,
        lockfile: &Lockfile,
    ) -> Lockfile {
        self.route_lockfile_until(config, lockfile, &|| false)
            .expect("unconditional tarball routing cannot cancel")
    }

    pub(in super::super) fn route_lockfile_until(
        &self,
        config: &PacquetConfig,
        lockfile: &Lockfile,
        should_cancel: &dyn Fn() -> bool,
    ) -> Option<Lockfile> {
        if should_cancel() {
            return None;
        }
        let mut routed = lockfile.clone();
        let Some(packages) = routed.packages.as_mut() else {
            return Some(routed);
        };
        for (package_key, metadata) in packages {
            if should_cancel() {
                return None;
            }
            self.route_package_metadata(config, package_key, metadata);
        }
        Some(routed)
    }

    fn route_package_metadata(
        &self,
        config: &PacquetConfig,
        package_key: &PackageKey,
        metadata: &mut PackageMetadata,
    ) {
        if !matches!(
            metadata.resolution,
            LockfileResolution::Registry(_) | LockfileResolution::Tarball(_),
        ) {
            return;
        }
        // A resolution that pins no integrity keeps its original URL:
        // routing it through the endpoint would hand the client a
        // mirrored tarball it has no hash to check.
        let Ok((tarball_url, Some(integrity))) =
            tarball_url_and_integrity(&metadata.resolution, package_key, config)
        else {
            return;
        };
        if !is_http_tarball_url(&tarball_url) || is_git_hosted_tarball_url(&tarball_url) {
            return;
        }
        let routed_url =
            self.route_lockfile_tarball(config, package_key, &metadata.resolution, &tarball_url);
        if routed_url == tarball_url.as_ref() {
            return;
        }
        metadata.resolution = LockfileResolution::Tarball(TarballResolution {
            tarball: routed_url,
            integrity: Some(integrity.clone()),
            revision: None,
            git_hosted: None,
            path: None,
        });
    }

    pub(in super::super) fn verification_lockfile(&self, lockfile: &Lockfile) -> Lockfile {
        let mut upstream = lockfile.clone();
        let Some(packages) = upstream.packages.as_mut() else {
            return upstream;
        };
        for (package_key, metadata) in packages {
            let LockfileResolution::Tarball(resolution) = &mut metadata.resolution else {
                continue;
            };
            let name = package_key.name.to_string();
            if let Some(tarball_url) =
                self.upstream_endpoint_tarball_url(&resolution.tarball, Some(&name))
            {
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
            RouteClass::Hosted { .. } => {
                pnpr_tarball_url(&self.public_url, package, version, tarball_url)
            }
            RouteClass::Proxied { alias, .. } => upstream_endpoint_tarball_url(
                &self.public_url,
                &alias,
                package,
                version,
                tarball_url,
            ),
        }
    }

    /// Reverse a named endpoint tarball URL back to its
    /// upstream URL so an input lockfile carrying endpoint URLs can be verified
    /// against the real registry. Returns `None` for any other URL, and for an
    /// endpoint the caller is not authorized for (so verification cannot be
    /// used as an oracle for an upstream the caller cannot reach).
    pub(super) fn upstream_endpoint_tarball_url(
        &self,
        tarball_url: &str,
        package: Option<&str>,
    ) -> Option<String> {
        let prefix = format!("{}/~", npm_endpoint(&self.public_url));
        let route = tarball_url.strip_prefix(&prefix)?;
        let (upstream, rest) = route.split_once('/')?;
        let registry = self.context.upstream_registry(&self.identity, upstream, package)?;
        Some(format!("{}/{rest}", registry.trim_end_matches('/')))
    }
}

fn is_registry_package_resolution(
    package_key: &PackageKey,
    resolution: &LockfileResolution,
) -> bool {
    package_key.suffix.prefix() == pnpm_lockfile::Prefix::None
        && package_key.suffix.registry_version().is_some()
        && match resolution {
            LockfileResolution::Registry(_) => true,
            LockfileResolution::Tarball(tarball) => {
                is_http_tarball_url(&tarball.tarball)
                    && !is_git_hosted_tarball_url(&tarball.tarball)
            }
            _ => false,
        }
}

fn hint_registry_prefix<'a>(id: &'a str, package: &str) -> Option<&'a str> {
    let suffix = id.strip_prefix(package)?.strip_prefix('@')?;
    let version_part = suffix
        .split('(')
        .next()
        .unwrap_or(suffix);
    version_part.split_once(':').map(|(prefix, _)| prefix)
}

pub(super) fn tarball_route_tail(package: &str, version: &str, tarball_url: &str) -> String {
    if let Ok(name) = CanonicalPackageName::parse(package, pnpr_package_name::Ecosystem::Npm)
        && let Some(route_version) = version_route_tarball_version(tarball_url, &name)
    {
        return format!("{package}@{route_version}");
    }
    let filename = match CanonicalPackageName::parse(package, pnpr_package_name::Ecosystem::Npm) {
        Ok(name) => tarball_basename_for_package(tarball_url, &name)
            .map_or_else(|| name.tarball_name_for_version(version), std::borrow::Cow::into_owned),
        Err(_) => tarball_basename(tarball_url)
            .map_or_else(|| format!("{package}-{version}.tgz"), str::to_string),
    };
    format!("{package}/-/{filename}")
}

pub(super) fn pnpr_tarball_url(
    public_url: &str,
    package: &str,
    version: &str,
    tarball_url: &str,
) -> String {
    format!("{}/{}", npm_endpoint(public_url), tarball_route_tail(package, version, tarball_url))
}

/// The registry-endpoint URL a proxied route's tarball is served through.
pub(super) fn upstream_endpoint_tarball_url(
    public_url: &str,
    upstream: &str,
    package: &str,
    version: &str,
    tarball_url: &str,
) -> String {
    format!(
        "{}/~{upstream}/{}",
        npm_endpoint(public_url),
        tarball_route_tail(package, version, tarball_url),
    )
}

fn npm_endpoint(public_url: &str) -> String {
    format!("{}{}", public_url.trim_end_matches('/'), pnpr_registry::Ecosystem::Npm.base_path())
}
