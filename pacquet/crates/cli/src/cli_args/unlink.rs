use crate::State;
use clap::Args;
use miette::Context;
use pacquet_config::Config;
use pacquet_package_manager::{Install, UpdateSeedPolicy};
use pacquet_package_manifest::DependencyGroup;
use pacquet_reporter::Reporter;
use pacquet_workspace_manifest_writer::remove_overrides;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

/// Removes the link created by `pacquet link` and reinstalls the package if
/// it is saved in `package.json`.
///
/// Mirrors pnpm's `unlink`: it strips `link:` entries from the `overrides`
/// block in `pnpm-workspace.yaml` (the same source the installer reads), then
/// runs install so the previous resolution is restored. With package names it
/// only drops the matching `link:` overrides; with none it drops them all.
#[derive(Debug, Args)]
pub struct UnlinkArgs {
    pub package_names: Vec<String>,
}

impl UnlinkArgs {
    pub async fn run<Reporter: self::Reporter + 'static>(
        self,
        config: &'static mut Config,
        manifest_path: PathBuf,
    ) -> miette::Result<()> {
        // pnpm prints "Nothing to unlink" and stops when no overrides are
        // configured; otherwise it strips the matching link: overrides and
        // reinstalls — running the install even when nothing matched.
        let Some(overrides) = config.overrides.as_mut() else {
            println!("Nothing to unlink");
            return Ok(());
        };

        let removed: Vec<String> = overrides
            .iter()
            .filter(|(selector, specifier)| {
                specifier.starts_with("link:")
                    && (self.package_names.is_empty()
                        || self.package_names.iter().any(|name| name == *selector))
            })
            .map(|(selector, _)| selector.clone())
            .collect();

        for selector in &removed {
            overrides.shift_remove(selector);
        }

        if !removed.is_empty() {
            let root_dir = config
                .workspace_dir
                .clone()
                .or_else(|| manifest_path.parent().map(Path::to_path_buf))
                .ok_or_else(|| miette::miette!("manifest path has no parent directory"))?;

            remove_overrides(&root_dir, &removed)
                .wrap_err("removing link: overrides from pnpm-workspace.yaml")?;
        }

        let state = State::init(manifest_path, config, false).wrap_err("initialize the state")?;

        let State { tarball_mem_cache, http_client, config, manifest, lockfile, resolved_packages } =
            &state;

        let lockfile_path = manifest
            .path()
            .parent()
            .map(|parent| parent.join(pacquet_lockfile::Lockfile::FILE_NAME));

        Install {
            tarball_mem_cache: Arc::clone(tarball_mem_cache),
            http_client,
            http_client_arc: Arc::clone(http_client),
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
            prefer_frozen_lockfile: Some(false),
            ignore_manifest_check: false,
            skip_runtimes: config.skip_runtimes,
            trust_lockfile: config.trust_lockfile,
            update_checksums: false,
            is_full_install: false,
            resolved_packages,
            supported_architectures: config.supported_architectures.clone(),
            node_linker: config.node_linker,
            lockfile_only: false,
            dry_run: false,
            disable_optimistic_repeat_install: false,
            pnpmfile_hook_override: None,
            workspace_projects_override: None,
            update_seed_policy: UpdateSeedPolicy::KeepAll,
            auth_override: None,
            resolution_observer: None,
            catalogs_override: None,
        }
        .run::<Reporter>()
        .await
        .wrap_err("unlinking dependencies")
    }
}
