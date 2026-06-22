use crate::State;
use clap::Args;
use miette::Context;
use pacquet_package_manager::Install;
use pacquet_package_manifest::DependencyGroup;
use pacquet_reporter::Reporter;

#[derive(Debug, Args)]
pub struct UnlinkArgs {
    pub package_names: Vec<String>,
}

impl UnlinkArgs {
    pub async fn run<Reporter: self::Reporter + 'static>(
        self,
        mut state: State,
    ) -> miette::Result<()> {
        let packages_to_remove: Vec<String> = if self.package_names.is_empty() {
            state
                .manifest
                .dependencies([
                    DependencyGroup::Prod,
                    DependencyGroup::Dev,
                    DependencyGroup::Optional,
                ])
                .filter(|(_, version)| version.starts_with("link:"))
                .map(|(name, _)| name.to_string())
                .collect()
        } else {
            self.package_names.clone()
        };

        if packages_to_remove.is_empty() {
            return Ok(());
        }

        state.manifest.remove_dependencies(&packages_to_remove, None);
        state.manifest.save().wrap_err("saving package.json after unlinking")?;

        let State { tarball_mem_cache, http_client, config, manifest, lockfile, resolved_packages } =
            &state;

        let lockfile_path = manifest
            .path()
            .parent()
            .map(|parent| parent.join(pacquet_lockfile::Lockfile::FILE_NAME));

        Install {
            tarball_mem_cache: std::sync::Arc::clone(tarball_mem_cache),
            http_client,
            http_client_arc: std::sync::Arc::clone(http_client),
            config,
            manifest,
            lockfile: pacquet_lockfile::MaybeLazyLockfile::Lazy(lockfile),
            lockfile_path: lockfile_path.as_deref(),
            dependency_groups: [
                DependencyGroup::Prod,
                DependencyGroup::Dev,
                DependencyGroup::Optional,
            ]
            .into_iter(),
            frozen_lockfile: false,
            prefer_frozen_lockfile: None,
            ignore_manifest_check: false,
            skip_runtimes: false,
            trust_lockfile: false,
            update_checksums: false,
            is_full_install: true,
            resolved_packages,
            supported_architectures: config.supported_architectures.clone(),
            node_linker: config.node_linker,
            lockfile_only: false,
            dry_run: false,
            update_seed_policy: pacquet_package_manager::UpdateSeedPolicy::KeepAll,
            auth_override: None,
            resolution_observer: None,
            catalogs_override: None,
        }
        .run::<Reporter>()
        .await
        .wrap_err("unlinking dependencies")
    }
}
