use pnpm_config::PackageImportMethod;
use pnpm_store_dir::{SharedReadonlyStoreIndex, StoreDir, StoreIndexWriter};
use std::{collections::HashMap, path::Path, sync::Arc};

/// Default npm registry used when neither the config nor a scope entry
/// names one.
const DEFAULT_REGISTRY: &str = "https://registry.npmjs.org/";

/// Handles and settings the config-dependency resolve/install pass
/// needs. Assembled by the caller (the config-finalization seam) from
/// the resolved [`pnpm_config::Config`] plus a network client, then
/// passed by reference into [`crate::resolve_and_install_config_deps()`].
///
/// Long-lived network/config handles remain borrowed from the caller. Store-index
/// handles are shared so tarball materialization can read and persist cache rows.
pub struct ConfigDepsInstallOptions<'a> {
    pub verification: ConfigDependencyVerification<'a>,
    pub fetching: pnpm_tarball::ArchiveFetchOptions<'a>,
    pub platform: pnpm_package_is_installable::InstallabilityOptions<'a>,
    pub store: crate::ConfigDependencyStore,
    pub store_index: Option<SharedReadonlyStoreIndex>,
    pub store_index_writer: Option<Arc<StoreIndexWriter>>,
    /// `lockfileDir` — where `pnpm-lock.yaml` and
    /// `node_modules/.pnpm-config` live.
    pub root_dir: &'a Path,
    /// `--frozen-lockfile`: refuse to mutate the env lockfile.
    pub frozen_lockfile: bool,
}

pub struct ConfigDependencyVerification<'a> {
    /// `default` plus per-scope (`@scope`) registry entries.
    pub registries: &'a HashMap<String, String>,
    pub resolution_verifiers: Vec<Arc<dyn pnpm_resolving_resolver_base::ResolutionVerifier>>,
    /// Applied when resolving a config dependency, so a resolution never
    /// picks a version the resolution verifiers reject.
    pub resolution_policy: pnpm_resolving_resolver_base::ResolutionPolicyOptions,
}

impl ConfigDependencyVerification<'_> {
    /// The install-wide default registry. Used to derive a config
    /// dependency's tarball URL when the lockfile stored an
    /// integrity-only (registry-form) resolution.
    pub(crate) fn default_registry(&self) -> &str {
        self.registries.get("default").map_or(DEFAULT_REGISTRY, String::as_str)
    }

    /// Registry serving `name`: a scoped package consults its `@scope`
    /// entry, falling back to the default.
    pub(crate) fn pick_registry(&self, name: &str) -> &str {
        if let Some(scope_end) = scope_of(name)
            && let Some(registry) = self.registries.get(&name[..scope_end])
        {
            return registry;
        }
        self.default_registry()
    }
}

impl ConfigDepsInstallOptions<'_> {
    /// The `prefix`/`requester` string pnpm threads into fetch + log
    /// payloads — the install root.
    pub(crate) fn requester(&self) -> String {
        self.root_dir.to_string_lossy().into_owned()
    }
}

/// Byte offset just past the `@scope` of a scoped package name, or
/// `None` for an unscoped name.
fn scope_of(name: &str) -> Option<usize> {
    name.starts_with('@')
        .then(|| name.find('/'))
        .flatten()
}

#[derive(Clone, Copy)]
pub struct ConfigDependencyStore {
    pub dir: &'static StoreDir,
    pub verify_integrity: bool,
    pub strict_pkg_content_check: bool,
    pub package_import_method: PackageImportMethod,
}
