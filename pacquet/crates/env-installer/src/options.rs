use pacquet_config::PackageImportMethod;
use pacquet_network::{AuthHeaders, RetryOpts, ThrottledClient};
use pacquet_package_is_installable::SupportedArchitectures;
use pacquet_store_dir::StoreDir;
use std::{collections::HashMap, path::Path};

/// Default npm registry used when neither the config nor a scope entry
/// names one. Mirrors pnpm's `DEFAULT_REGISTRIES.default`.
const DEFAULT_REGISTRY: &str = "https://registry.npmjs.org/";

/// Handles and settings the config-dependency resolve/install pass
/// needs. Assembled by the caller (the config-finalization seam) from
/// the resolved [`pacquet_config::Config`] plus a network client, then
/// passed by reference into [`crate::resolve_and_install_config_deps()`].
///
/// Every field borrows so the caller keeps ownership of the long-lived
/// install handles (HTTP client, auth headers, registries map).
pub struct ConfigDepsInstallOptions<'a> {
    /// `lockfileDir` — where `pnpm-lock.yaml` and
    /// `node_modules/.pnpm-config` live.
    pub root_dir: &'a Path,
    pub store_dir: &'static StoreDir,
    pub http_client: &'a ThrottledClient,
    pub auth_headers: &'a AuthHeaders,
    /// `default` plus per-scope (`@scope`) registry entries.
    pub registries: &'a HashMap<String, String>,
    pub verify_store_integrity: bool,
    pub offline: bool,
    pub package_import_method: PackageImportMethod,
    pub retry_opts: RetryOpts,
    /// `--frozen-lockfile`: refuse to mutate the env lockfile.
    pub frozen_lockfile: bool,
    pub supported_architectures: Option<&'a SupportedArchitectures>,
    pub current_node_version: &'a str,
    pub current_os: &'a str,
    pub current_cpu: &'a str,
    pub current_libc: &'a str,
}

impl ConfigDepsInstallOptions<'_> {
    /// The install-wide default registry. Used to derive a config
    /// dependency's tarball URL when the lockfile stored an
    /// integrity-only (registry-form) resolution.
    pub(crate) fn default_registry(&self) -> &str {
        self.registries.get("default").map_or(DEFAULT_REGISTRY, String::as_str)
    }

    /// Registry serving `name`. Mirrors pnpm's
    /// [`pickRegistryForPackage`](https://github.com/pnpm/pnpm/blob/31858c544b/config/pick-registry-for-package/src/index.ts):
    /// a scoped package consults its `@scope` entry, falling back to
    /// the default.
    pub(crate) fn pick_registry(&self, name: &str) -> &str {
        if let Some(scope_end) = scope_of(name)
            && let Some(registry) = self.registries.get(&name[..scope_end])
        {
            return registry;
        }
        self.default_registry()
    }

    /// The `prefix`/`requester` string pnpm threads into fetch + log
    /// payloads — the install root.
    pub(crate) fn requester(&self) -> String {
        self.root_dir.to_string_lossy().into_owned()
    }
}

/// Byte offset just past the `@scope` of a scoped package name, or
/// `None` for an unscoped name.
fn scope_of(name: &str) -> Option<usize> {
    name.starts_with('@').then(|| name.find('/')).flatten()
}
