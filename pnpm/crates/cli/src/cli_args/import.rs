use crate::State;
use clap::Args;
use miette::{Context, IntoDiagnostic};
use pnpm_lockfile::EnvLockfile;
use pnpm_package_manager::{Install, ProjectMutation};
use pnpm_package_manifest::DependencyGroup;
use pnpm_reporter::Reporter;

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
        let env_lockfile = EnvLockfile::read(dir)
            .into_diagnostic()
            .wrap_err("reading the env lockfile before import")?;

        let lockfile_backup = lockfile_path.with_extension("yaml.import.bak");
        let lockfile_existed = lockfile_path.exists();
        if lockfile_existed {
            std::fs::rename(&lockfile_path, &lockfile_backup)
                .into_diagnostic()
                .wrap_err("backing up existing pnpm-lock.yaml")?;
        }
        let import_lockfile = pnpm_lockfile::LazyLockfile::preloaded(None);

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
                    update_patches: false,
                    fix_lockfile: false,
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
                lockfile: pnpm_lockfile::MaybeLazyLockfile::Lazy(&import_lockfile),
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
                mutation: ProjectMutation::NoInstall,
                installs_only: true,
                resolved_packages,
                supported_architectures: config.supported_architectures.clone(),
                node_linker: config.node_linker,
                lockfile_only: true,
                dry_run: false,
                persist_policy_excludes: false,
                update_seed_policy: pnpm_package_manager::UpdateSeedPolicy::drop_all(),
                preferred_versions_override: None,
                auth_override: None,
                resolution_observer: None,
                peer_issues_sink: None,
                deps_requiring_build_sink: None,
                catalogs_override: None,
                disable_optimistic_repeat_install: false,
                pnpmfile_hook_override: None,
                workspace_projects_override: None,
            }
            .run::<Reporter>()
            .await
            .wrap_err("importing dependencies")
        };

        let import_result = install_result.and_then(|()| {
            if let Some(env_lockfile) = env_lockfile {
                env_lockfile
                    .write(dir)
                    .into_diagnostic()
                    .wrap_err("preserving the env lockfile after import")?;
            }
            Ok(())
        });

        match import_result {
            Ok(()) => {
                if lockfile_existed {
                    std::fs::remove_file(&lockfile_backup)
                        .into_diagnostic()
                        .wrap_err("removing the import lockfile backup")?;
                }
                Ok(())
            }
            Err(error) => {
                if lockfile_existed {
                    restore_lockfile(&lockfile_path, &lockfile_backup).wrap_err_with(|| {
                        format!("restoring pnpm-lock.yaml after import failed: {error}")
                    })?;
                }
                Err(error)
            }
        }
    }
}

fn restore_lockfile(
    lockfile_path: &std::path::Path,
    backup_path: &std::path::Path,
) -> miette::Result<()> {
    if let Err(error) = std::fs::remove_file(lockfile_path)
        && error.kind() != std::io::ErrorKind::NotFound
    {
        return Err(error).into_diagnostic().wrap_err("removing the failed imported lockfile");
    }
    std::fs::rename(backup_path, lockfile_path)
        .into_diagnostic()
        .wrap_err("restoring the original pnpm-lock.yaml")
}
