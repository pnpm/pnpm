use std::path::Path;

use crate::State;
use clap::Args;
use miette::{Context, IntoDiagnostic};
use pacquet_package_manager::Install;
use pacquet_package_manifest::DependencyGroup;
use pacquet_reporter::Reporter;

#[derive(Debug, Args)]
pub struct DedupeArgs {
    #[clap(long)]
    pub check: bool,
}

impl DedupeArgs {
    pub async fn run<Reporter: self::Reporter + 'static>(
        self,
        state: State,
        existing: Option<String>,
        lockfile_path: &Path,
    ) -> miette::Result<()> {
        let State { tarball_mem_cache, http_client, config, manifest, lockfile, resolved_packages } =
            &state;

        Install {
            tarball_mem_cache: std::sync::Arc::clone(tarball_mem_cache),
            http_client,
            http_client_arc: std::sync::Arc::clone(http_client),
            config,
            manifest,
            lockfile: pacquet_lockfile::MaybeLazyLockfile::Lazy(lockfile),
            lockfile_path: Some(lockfile_path),
            dependency_groups: [
                DependencyGroup::Prod,
                DependencyGroup::Dev,
                DependencyGroup::Optional,
            ]
            .into_iter(),
            frozen_lockfile: false,
            prefer_frozen_lockfile: Some(false),
            ignore_manifest_check: false,
            skip_runtimes: false,
            trust_lockfile: false,
            update_checksums: false,
            is_full_install: true,
            resolved_packages,
            supported_architectures: config.supported_architectures.clone(),
            node_linker: config.node_linker,
            lockfile_only: true,
            dry_run: false,
            update_seed_policy: pacquet_package_manager::UpdateSeedPolicy::DropAll,
            auth_override: None,
            resolution_observer: None,
            catalogs_override: None,
        }
        .run::<Reporter>()
        .await
        .wrap_err("deduplicating dependencies")?;

        if self.check {
            let current = match std::fs::read_to_string(lockfile_path) {
                Ok(content) => Some(content),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
                Err(e) => {
                    return Err(e).into_diagnostic().wrap_err("reading lockfile after dedupe");
                }
            };
            if existing != current {
                if let Some(old) = existing {
                    std::fs::write(lockfile_path, &old)
                        .into_diagnostic()
                        .wrap_err("restoring lockfile after check")?;
                } else {
                    std::fs::remove_file(lockfile_path)
                        .into_diagnostic()
                        .wrap_err("removing lockfile after check")?;
                }
                return Err(miette::miette!("Lockfile would be modified by deduplication"));
            }
        }

        Ok(())
    }
}
