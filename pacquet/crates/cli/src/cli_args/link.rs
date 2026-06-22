use crate::State;
use clap::Args;
use miette::Context;
use pacquet_package_manager::Install;
use pacquet_package_manifest::{DependencyGroup, PackageManifest};
use pacquet_reporter::Reporter;
use std::path::PathBuf;

#[derive(Debug, Args)]
pub struct LinkArgs {
    pub package_paths: Vec<String>,
}

impl LinkArgs {
    pub async fn run<Reporter: self::Reporter + 'static>(
        self,
        mut state: State,
    ) -> miette::Result<()> {
        if self.package_paths.is_empty() {
            return Err(miette::miette!(
                "Cannot link by package name. Use a relative or absolute path instead."
            ));
        }

        let manifest_dir = state
            .manifest
            .path()
            .parent()
            .ok_or_else(|| miette::miette!("manifest path has no parent directory"))?
            .to_path_buf();

        for path_str in &self.package_paths {
            let target_path = PathBuf::from(path_str);
            let target_dir = if target_path.is_absolute() {
                target_path.clone()
            } else {
                manifest_dir.join(&target_path)
            };

            let target_manifest_path = target_dir.join("package.json");
            let dir_display = target_dir.display();
            let target_manifest =
                PackageManifest::from_path(target_manifest_path).map_err(|_| {
                    miette::miette!("No package.json found in {}", dir_display)
                })?;
            let package_name = target_manifest.value()["name"]
                .as_str()
                .ok_or_else(|| miette::miette!("Target package does not have a name field"))?
                .to_string();

            let normalized = pathdiff::diff_paths(&target_dir, &manifest_dir)
                .ok_or_else(|| miette::miette!("cannot compute relative path to target"))?;
            let link_spec = format!("link:{}", normalized.display());

            let manifest = &mut state.manifest;
            manifest
                .add_dependency(&package_name, &link_spec, DependencyGroup::Prod)
                .wrap_err("adding linked dependency to package.json")?;
        }

        state.manifest.save().wrap_err("saving package.json with linked dependencies")?;

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
        .wrap_err("linking dependencies")
    }
}
