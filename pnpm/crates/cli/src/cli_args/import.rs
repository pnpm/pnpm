use crate::State;
use clap::Args;
use miette::{Context, IntoDiagnostic};
use pacquet_package_manager::Install;
use pacquet_package_manifest::DependencyGroup;
use pacquet_reporter::Reporter;

#[derive(Debug, Args)]
pub struct ImportArgs {
    /// URL of a pnpr server to offload lockfile resolution to.
    #[clap(long = "pnpr-server")]
    pub pnpr_server: Option<String>,
}

impl ImportArgs {
    pub async fn run<Reporter: self::Reporter + 'static>(self, state: State) -> miette::Result<()> {
        let State { tarball_mem_cache, http_client, config, manifest, resolved_packages, .. } =
            &state;
        let dir = manifest.path().parent().expect("manifest path always has a parent dir");
        let lockfile_path = dir.join("pnpm-lock.yaml");

        let lockfile_backup = lockfile_path.with_extension("yaml.import.bak");
        let lockfile_existed = lockfile_path.exists();
        if lockfile_existed {
            std::fs::rename(&lockfile_path, &lockfile_backup)
                .into_diagnostic()
                .wrap_err("backing up existing pnpm-lock.yaml")?;
        }
        let import_lockfile = pacquet_lockfile::LazyLockfile::preloaded(None);

        let install_result = if let Some(pnpr_server) =
            self.pnpr_server.as_deref().or(config.pnpr_server.as_deref())
        {
            super::install::install_via_pnpr::<Reporter>(
                &state,
                pnpr_server,
                super::install::PnprLink {
                    dependency_groups: vec![
                        DependencyGroup::Prod,
                        DependencyGroup::Dev,
                        DependencyGroup::Optional,
                    ],
                    supported_architectures: config.supported_architectures.clone(),
                    node_linker: config.node_linker,
                    skip_runtimes: config.skip_runtimes,
                    frozen_lockfile: false,
                    prefer_frozen_lockfile: false,
                    lockfile_only: true,
                    ignore_manifest_check: false,
                    trust_lockfile: false,
                    lockfile_path: Some(lockfile_path.as_path()),
                    use_state_lockfile: false,
                },
            )
            .await
            .wrap_err("importing dependencies via the pnpr server")
        } else {
            Install {
                tarball_mem_cache: std::sync::Arc::clone(tarball_mem_cache),
                http_client,
                http_client_arc: std::sync::Arc::clone(http_client),
                config,
                manifest,
                emit_initial_manifest: true,
                lockfile: pacquet_lockfile::MaybeLazyLockfile::Lazy(&import_lockfile),
                lockfile_path: Some(lockfile_path.as_path()),
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
                trust_lockfile: false,
                update_checksums: false,
                is_full_install: false,
                installs_only: true,
                resolved_packages,
                supported_architectures: config.supported_architectures.clone(),
                node_linker: config.node_linker,
                lockfile_only: true,
                dry_run: false,
                update_seed_policy: pacquet_package_manager::UpdateSeedPolicy::DropAll,
                auth_override: None,
                resolution_observer: None,
                peer_issues_sink: None,
                catalogs_override: None,
                disable_optimistic_repeat_install: false,
                pnpmfile_hook_override: None,
                workspace_projects_override: None,
            }
            .run::<Reporter>()
            .await
            .wrap_err("importing dependencies")
        };

        match install_result {
            Ok(()) => {
                if lockfile_existed {
                    let _ = std::fs::remove_file(&lockfile_backup);
                }
                Ok(())
            }
            Err(error) => {
                if lockfile_existed {
                    let _ = std::fs::rename(&lockfile_backup, &lockfile_path);
                }
                Err(error)
            }
        }
    }
}
