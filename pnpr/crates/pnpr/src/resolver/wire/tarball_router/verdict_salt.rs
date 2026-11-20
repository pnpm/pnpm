use std::fmt::Write as _;

use pnpm_lockfile::PkgName;
use sha2::{
    Digest as _,
    Sha256,
};

use super::{
    BUILTIN_REGISTRIES_BY_PREFIX,
    HashMap,
    Lockfile,
    LockfileResolution,
    PackageKey,
    PacquetConfig,
    RouteClass,
    TarballRouter,
    VerificationRegistryRoutes,
};

const VERDICT_HASH_VERSION: &[u8] = b"pnpr-verdict-v3";
const CANCEL_CHECK_ENTRY_STRIDE: usize = 256;

impl TarballRouter {
    #[cfg(test)]
    pub(in crate::resolver) fn verification_verdict_hash(
        &self,
        config: &PacquetConfig,
        lockfile: &Lockfile,
    ) -> String {
        self.verification_verdict_hash_until(config, lockfile, &|| false)
            .expect("unconditional verdict hashing cannot cancel")
    }

    /// Compute an identity- and routing-scoped verdict cache key without
    /// cloning `lockfile` or building a package-by-registry cross product.
    /// Hashes each entry's single selected registry classification and any
    /// authorized `/~<alias>/` reversal into a fixed 32-byte digest.
    pub(in crate::resolver) fn verification_verdict_hash_until(
        &self,
        config: &PacquetConfig,
        lockfile: &Lockfile,
        should_cancel: &dyn Fn() -> bool,
    ) -> Option<String> {
        if should_cancel() {
            return None;
        }
        let routes = self.build_verification_routes(config);
        let entry_digests = self.collect_entry_binding_digests(&routes, lockfile, should_cancel)?;
        if should_cancel() {
            return None;
        }
        let raw_hash = pnpm_lockfile_verification::hash_lockfile(lockfile);
        let mut hasher = Sha256::new();
        hasher.update(VERDICT_HASH_VERSION);
        hasher.update([0]);
        hasher.update(raw_hash.as_bytes());
        hasher.update([0]);
        hasher.update(self.public_url.as_bytes());
        for digest in &entry_digests {
            hasher.update([0]);
            hasher.update(digest);
        }
        Some(format!("{:x}", hasher.finalize()))
    }

    pub(in crate::resolver::wire) fn build_verification_routes(
        &self,
        config: &PacquetConfig,
    ) -> VerificationRegistryRoutes {
        let registries: HashMap<String, String> = if self.registries.is_empty() {
            config
                .resolved_registries()
                .into_iter()
                .collect()
        } else {
            self.registries
                .iter()
                .map(|(scope, url)| {
                    (scope.clone(), pnpm_network::normalize_registry_url(url).into_owned())
                })
                .collect()
        };
        let mut registries_by_prefix: HashMap<String, String> = BUILTIN_REGISTRIES_BY_PREFIX
            .iter()
            .map(|(alias, url)| {
                ((*alias).to_string(), pnpm_network::normalize_registry_url(url).into_owned())
            })
            .collect();
        for (alias, url) in &self.registries_by_prefix {
            registries_by_prefix.insert(
                alias.clone(),
                pnpm_network::normalize_registry_url(url).into_owned(),
            );
        }
        for (alias, url) in &config.registries_by_prefix {
            registries_by_prefix.insert(
                alias.clone(),
                pnpm_network::normalize_registry_url(url).into_owned(),
            );
        }
        VerificationRegistryRoutes::new(registries, registries_by_prefix, None)
    }

    /// Select the registry URL that [`pnpm_resolving_npm_resolver::NpmResolutionVerifier`]
    /// verified `package_key` / `resolution` against, reversing authorized
    /// `/~<alias>/` endpoints first.
    pub(in crate::resolver::wire) fn verified_entry_registry(
        &self,
        routes: &VerificationRegistryRoutes,
        package_key: &PackageKey,
        resolution: &LockfileResolution,
    ) -> Option<String> {
        let name = package_key.name.to_string();
        if let Some((registry_name, _)) = package_key.suffix.registry_qualified() {
            return routes.select_registry(&package_key.name, Some(registry_name), None);
        }
        let effective = match resolution {
            LockfileResolution::Tarball(tarball) => self
                .upstream_endpoint_tarball_url(&tarball.tarball, Some(&name))
                .or_else(|| Some(tarball.tarball.clone())),
            _ => None,
        };
        routes.select_registry(&package_key.name, None, effective.as_deref())
    }

    fn collect_entry_binding_digests(
        &self,
        routes: &VerificationRegistryRoutes,
        lockfile: &Lockfile,
        should_cancel: &dyn Fn() -> bool,
    ) -> Option<Vec<[u8; 32]>> {
        let Some(packages) = lockfile.packages.as_ref() else {
            return Some(Vec::new());
        };
        let mut digests = Vec::with_capacity(packages.len());
        let mut scratch = String::with_capacity(256);
        for (index, (package_key, metadata)) in packages.iter().enumerate() {
            if index % CANCEL_CHECK_ENTRY_STRIDE == 0 && should_cancel() {
                return None;
            }
            digests.push(self.hash_entry_binding(
                routes,
                package_key,
                &metadata.resolution,
                &mut scratch,
            ));
        }
        digests.sort_unstable();
        Some(digests)
    }

    fn hash_entry_binding(
        &self,
        routes: &VerificationRegistryRoutes,
        package_key: &PackageKey,
        resolution: &LockfileResolution,
        scratch: &mut String,
    ) -> [u8; 32] {
        scratch.clear();
        let _ = write!(scratch, "{package_key}");
        let name_str = package_key.name.to_string();
        let registry_name = package_key.suffix.registry_qualified().map(|(reg_name, _)| reg_name);
        self.append_resolution_binding(
            routes,
            (&package_key.name, &name_str, registry_name),
            resolution,
            scratch,
        );
        Sha256::digest(scratch.as_bytes()).into()
    }

    fn append_resolution_binding(
        &self,
        routes: &VerificationRegistryRoutes,
        (pkg_name, name_str, registry_name): (&PkgName, &str, Option<&str>),
        resolution: &LockfileResolution,
        scratch: &mut String,
    ) {
        match resolution {
            LockfileResolution::Registry(_) => {
                self.append_selected_registry(
                    routes,
                    (pkg_name, name_str, registry_name),
                    None,
                    scratch,
                );
            }
            LockfileResolution::Tarball(tarball) => {
                let reversed = self.upstream_endpoint_tarball_url(&tarball.tarball, Some(name_str));
                let effective = reversed.as_deref().unwrap_or(tarball.tarball.as_str());
                self.append_selected_registry(
                    routes,
                    (pkg_name, name_str, registry_name),
                    Some(effective),
                    scratch,
                );
                scratch.push('\x1f');
                if let Some(reversed) = reversed.as_deref() {
                    scratch.push_str(reversed);
                }
            }
            LockfileResolution::Variations(variations) => {
                for variant in &variations.variants {
                    self.append_resolution_binding(
                        routes,
                        (pkg_name, name_str, registry_name),
                        &variant.resolution,
                        scratch,
                    );
                }
            }
            _ => {}
        }
    }

    fn append_selected_registry(
        &self,
        routes: &VerificationRegistryRoutes,
        (pkg_name, name_str, registry_name): (&PkgName, &str, Option<&str>),
        effective_tarball_url: Option<&str>,
        scratch: &mut String,
    ) {
        match routes.select_registry(pkg_name, registry_name, effective_tarball_url) {
            Some(registry) => self.append_binding_tag(scratch, &registry, name_str),
            None => scratch.push_str("\x1emissing"),
        }
    }

    fn append_binding_tag(&self, line: &mut String, registry: &str, package: &str) {
        line.push('\x1e');
        match self.context.classify(&self.identity, registry, Some(package)) {
            RouteClass::Public => {
                let _ = write!(line, "pub:{registry}");
            }
            RouteClass::Hosted { policy_id } => {
                let _ = write!(line, "hosted:{registry}:{policy_id}");
            }
            RouteClass::Proxied { alias, credential_digest } => {
                let upstream = self.context
                    .upstream_registry(&self.identity, &alias, Some(package))
                    .unwrap_or_default();
                let _ = write!(line, "proxied:{registry}:{alias}:{credential_digest}:{upstream}");
            }
        }
    }
}
