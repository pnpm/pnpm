//! The context for resolving and installing config dependencies: the
//! network, the resolver, the store and the lockfile options, built from
//! the project's configuration or from the package-manager bootstrap
//! configuration.

use super::network::EnvironmentNetwork;
use miette::{IntoDiagnostic, Result, WrapErr};
use pnpm_config::Config;
use pnpm_env_installer::{
    ConfigDependencyVerification, ConfigDepsInstallMode, ConfigDepsInstallOptions,
};
use pnpm_graph_hasher::{detect_node_version, host_arch, host_libc, host_platform};
use pnpm_lockfile::EnvDocumentFormat;
use pnpm_network::ThrottledClient;
use pnpm_resolving_npm_resolver::{InMemoryPackageMetaCache, NpmResolver};
use pnpm_resolving_resolver_base::ResolutionPolicyOptions;
use std::{collections::HashMap, path::Path, sync::Arc};

pub(super) struct EnvInstallerContext {
    pub(super) node_version: String,
    pub(super) resolver: NpmResolver<InMemoryPackageMetaCache>,
    pub(super) network: EnvironmentNetwork,
    pub(super) store: pnpm_env_installer::ConfigDependencyStore,
    pub(super) symlinked_lockfile_target_dirs: Vec<std::path::PathBuf>,
}

impl EnvInstallerContext {
    pub(super) fn resolution_verifiers(
        &self,
        config: &Config,
    ) -> Result<Vec<Arc<dyn pnpm_resolving_resolver_base::ResolutionVerifier>>> {
        pnpm_package_manager::build_resolution_verifiers(
            config,
            Arc::clone(&self.network.http_client),
            None,
            Some(Arc::clone(&self.network.auth_headers)),
            None,
            None,
            None,
        )
        .into_diagnostic()
    }

    /// Context for resolving the project's `configDependencies`, using the
    /// project's configured registries and network settings.
    pub(super) fn new(config: &Config) -> Result<Self> {
        Self::build(
            config,
            &config.proxy,
            &config.tls,
            &config.tls_by_uri,
            config.resolved_registries(),
            Arc::clone(&config.auth_headers),
        )
    }

    /// Context for resolving the package manager pnpm auto-switches to
    /// (`pnpm` / `@pnpm/exe`), routed through the trusted
    /// [`PackageManagerBootstrap`](pnpm_config::PackageManagerBootstrap)
    /// config instead of the repository-controlled project registries.
    pub(super) fn for_package_manager(config: &Config) -> Result<Self> {
        let bootstrap = &config.package_manager_bootstrap;
        Self::build(
            config,
            &bootstrap.proxy,
            &bootstrap.tls,
            &bootstrap.tls_by_uri,
            bootstrap.resolved_registries(),
            Arc::clone(&bootstrap.auth_headers),
        )
    }

    fn build(
        config: &Config,
        proxy: &pnpm_network::ProxyConfig,
        tls: &pnpm_network::TlsConfig,
        tls_by_uri: &pnpm_network::PerRegistryTls,
        registries: std::collections::BTreeMap<String, String>,
        auth_headers: Arc<pnpm_network::AuthHeaders>,
    ) -> Result<Self> {
        let http_client = Arc::new(
            ThrottledClient::for_installs(proxy, tls, tls_by_uri, &config.network_settings())
                .into_diagnostic()
                .wrap_err("create the network client for env-installer dependencies")?
                .with_max_sockets_per_host(config.max_sockets),
        );

        let registries: HashMap<String, String> = registries.into_iter().collect();
        let retry_opts = config.retry_opts();
        let network = EnvironmentNetwork {
            http_client,
            auth_headers,
            registries,
            retry_opts,
            offline: config.offline,
            tar_limits: config.tar_limits(),
        };
        let resolver = network.resolver(config);

        Ok(Self {
            node_version: detect_node_version().unwrap_or_else(|| "0.0.0".to_string()),
            resolver,
            network,
            store: pnpm_env_installer::ConfigDependencyStore {
                dir: Box::leak(Box::new(config.store_dir.clone())),
                verify_integrity: config.verify_store_integrity,
                strict_pkg_content_check: config.strict_store_pkg_content_check,
                package_import_method: config.package_import_method,
            },
            symlinked_lockfile_target_dirs: config
                .symlinked_lockfile_target_dirs
                .clone(),
        })
    }

    /// The options for an install into `root_dir`, writing the env document
    /// in `format`, the pinned pnpm's.
    pub(super) fn options<'a>(
        &'a self,
        root_dir: &'a Path,
        frozen_lockfile: bool,
        format: EnvDocumentFormat,
    ) -> ConfigDepsInstallOptions<'a> {
        ConfigDepsInstallOptions {
            mode: ConfigDepsInstallMode::Reconcile,
            verification: ConfigDependencyVerification {
                registries: &self.network.registries,
                resolution_verifiers: Vec::new(),
                resolution_policy: ResolutionPolicyOptions::default(),
            },
            fetching: pnpm_tarball::ArchiveFetchOptions {
                http_client: &self.network.http_client,
                auth_headers: &self.network.auth_headers,
                retry_opts: self.network.retry_opts,
                offline: self.network.offline,
                tar_limits: self.network.tar_limits,
            },
            platform: pnpm_package_is_installable::InstallabilityOptions {
                supported_architectures: None,
                current_node_version: &self.node_version,
                current_os: host_platform(),
                current_cpu: host_arch(),
                current_libc: host_libc(),
                ..Default::default()
            },
            store: self.store,
            store_index: pnpm_env_installer::ConfigDependencyStoreIndex::default(),
            root_dir,
            lockfile: pnpm_env_installer::EnvLockfileOptions {
                frozen: frozen_lockfile,
                symlinked: pnpm_lockfile::SymlinkedLockfile::allowing(
                    &self.symlinked_lockfile_target_dirs,
                ),
                format,
            },
        }
    }
}
