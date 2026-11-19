use std::fmt::Write as _;

use sha2::{Digest as _, Sha256};

use super::{Lockfile, LockfileResolution, PacquetConfig, RouteClass, TarballRouter};

const VERDICT_HASH_VERSION: &[u8] = b"pnpr-verdict-v2";

impl TarballRouter {
    /// Compute an identity- and routing-scoped verdict cache key without
    /// cloning `lockfile`. Distinguishes binding rule versions, `public_url`,
    /// per-package `RouteClass` decisions (`Public` vs `Hosted` vs `Proxied`),
    /// and caller-authorized `/~<alias>/` reversals.
    pub(in crate::resolver) fn verification_verdict_hash(
        &self,
        config: &PacquetConfig,
        lockfile: &Lockfile,
    ) -> String {
        let raw_hash = pnpm_lockfile_verification::hash_lockfile(lockfile);
        let mut hasher = Sha256::new();
        hasher.update(VERDICT_HASH_VERSION);
        hasher.update([0]);
        hasher.update(raw_hash.as_bytes());
        hasher.update([0]);
        hasher.update(self.public_url.as_bytes());

        let mut candidate_registries: Vec<String> = config
            .resolved_registries()
            .into_values()
            .chain(config.registries_by_prefix.values().cloned())
            .chain(self.registries.values().cloned())
            .chain(self.registries_by_prefix.values().cloned())
            .collect();
        candidate_registries.sort_unstable();
        candidate_registries.dedup();

        let mut entries =
            self.collect_entry_binding_records(config, lockfile, &candidate_registries);
        entries.sort_unstable();
        for entry in entries {
            hasher.update([0]);
            hasher.update(entry.as_bytes());
        }
        format!("{:x}", hasher.finalize())
    }

    fn collect_entry_binding_records(
        &self,
        config: &PacquetConfig,
        lockfile: &Lockfile,
        candidate_registries: &[String],
    ) -> Vec<String> {
        let Some(packages) = lockfile.packages.as_ref() else {
            return Vec::new();
        };
        let mut records = Vec::with_capacity(packages.len());
        for (package_key, metadata) in packages {
            let name = package_key.name.to_string();
            let mut line = format!("{package_key}");
            let primary_registry = match package_key.suffix.registry_qualified() {
                Some((reg_name, _)) => self
                    .named_registry_url(Some(config), reg_name)
                    .map(str::to_string),
                None => Some(self.scope_registry_for_package(config, &name)),
            };
            if let Some(reg) = primary_registry.as_deref() {
                self.append_binding_tag(&mut line, reg, &name);
            }
            for reg in candidate_registries {
                self.append_binding_tag(&mut line, reg, &name);
            }
            self.append_resolution_reversals(&mut line, &name, &metadata.resolution);
            records.push(line);
        }
        records
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

    fn append_resolution_reversals(
        &self,
        line: &mut String,
        package: &str,
        resolution: &LockfileResolution,
    ) {
        match resolution {
            LockfileResolution::Tarball(tarball) => {
                let reversed = self
                    .upstream_endpoint_tarball_url(&tarball.tarball, Some(package))
                    .unwrap_or_default();
                line.push('\x1f');
                line.push_str(&reversed);
            }
            LockfileResolution::Variations(variations) => {
                for variant in &variations.variants {
                    self.append_resolution_reversals(line, package, &variant.resolution);
                }
            }
            _ => {}
        }
    }
}
