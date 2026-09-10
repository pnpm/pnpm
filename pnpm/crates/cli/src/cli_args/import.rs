use crate::State;
use clap::Args;
use miette::{Context, IntoDiagnostic};
use pnpm_lockfile::{EnvLockfile, Lockfile};
use pnpm_lockfile_import::{read_foreign_lockfile_versions, to_preferred_versions};
use pnpm_network::redact_url_for_display;
use pnpm_package_manager::{Install, ProjectMutation};
use pnpm_package_manifest::DependencyGroup;
use pnpm_reporter::Reporter;

#[derive(Debug, Args)]
pub struct ImportArgs {
    /// URL of a pnpr server. Accepted for symmetry with the other
    /// installing commands; `pnpm import` always resolves locally.
    // TODO: offloading import to pnpr requires uploading the lockfile or the
    // preferred versions it yields. Worth a follow up, but for now, since
    // import is an infrequent command, resolving locally is okay.
    #[clap(long = "pnpr-server")]
    pub pnpr_server: Option<String>,
}

impl ImportArgs {
    pub async fn run<Reporter: self::Reporter + 'static>(self, state: State) -> miette::Result<()> {
        let dir = state.manifest.path().parent().expect("manifest path always has a parent dir");
        let lockfile_dir = state.lockfile_dir();
        let lockfile_path = state.lockfile_path();
        let env_lockfile = if state.config.wanted_lockfile_name() == Lockfile::FILE_NAME {
            EnvLockfile::read(lockfile_dir)
                .into_diagnostic()
                .wrap_err("reading the env lockfile before import")?
        } else {
            None
        };

        if let Some(pnpr_server) =
            self.pnpr_server.as_deref().or(state.config.pnpr_server.as_deref())
        {
            let pnpr_server = redact_url_for_display(pnpr_server);
            pnpm_reporter::emit_global_warning::<Reporter>(&format!(
                r#""pnpm import" resolves dependencies locally, so the pnpr server at {pnpr_server} is not used"#,
            ));
        }

        let preferred_versions = to_preferred_versions(&read_foreign_lockfile_versions(dir)?);

        // A backup of its own keeps overlapping imports from restoring each
        // other's copy.
        let lockfile_backup =
            lockfile_path.with_extension(format!("yaml.{}.import.bak", std::process::id()));
        let lockfile_existed = lockfile_path.exists();
        if lockfile_existed {
            std::fs::rename(&lockfile_path, &lockfile_backup)
                .into_diagnostic()
                .wrap_err("backing up existing pnpm-lock.yaml")?;
        }
        let install_result =
            import_versions::<Reporter>(&state, &lockfile_path, preferred_versions).await;

        let import_result = install_result.and_then(|()| {
            if let Some(env_lockfile) = env_lockfile {
                env_lockfile
                    .write(lockfile_dir)
                    .into_diagnostic()
                    .wrap_err("preserving the env lockfile after import")?;
            }
            Ok(())
        });

        finish_import(import_result, &lockfile_path, &lockfile_backup, lockfile_existed)
    }
}

/// Restores the destination an import replaced. An import that started
/// without a lockfile there leaves none behind.
fn discard_failed_import(
    lockfile_path: &std::path::Path,
    backup_path: Option<&std::path::Path>,
) -> miette::Result<()> {
    if let Err(error) = std::fs::remove_file(lockfile_path)
        && error.kind() != std::io::ErrorKind::NotFound
    {
        return Err(error).into_diagnostic().wrap_err("removing the failed imported lockfile");
    }
    let Some(backup_path) = backup_path else { return Ok(()) };
    std::fs::rename(backup_path, lockfile_path)
        .into_diagnostic()
        .wrap_err("restoring the original lockfile")
}

async fn import_versions<Reporter: self::Reporter + 'static>(
    state: &State,
    lockfile_path: &std::path::Path,
    preferred_versions: pnpm_resolving_resolver_base::PreferredVersions,
) -> miette::Result<()> {
    let import_lockfile = pnpm_lockfile::LazyLockfile::preloaded(None);

    Install {
        lockfile_path: Some(lockfile_path),
        prefer_frozen_lockfile: Some(false),
        trust_lockfile: false,
        mutation: ProjectMutation::NoInstall,
        lockfile_only: true,
        update_seed_policy: pnpm_package_manager::UpdateSeedPolicy::drop_all(),
        preferred_versions_override: Some(preferred_versions),
        ..Install::new(
            std::sync::Arc::clone(&state.tarball_mem_cache),
            &state.resolved_packages,
            (&state.http_client, std::sync::Arc::clone(&state.http_client)),
            state.config,
            &state.manifest,
            pnpm_lockfile::MaybeLazyLockfile::Lazy(&import_lockfile),
            [DependencyGroup::Prod, DependencyGroup::Dev, DependencyGroup::Optional].into_iter(),
        )
    }
    .run::<Reporter>()
    .await
    .wrap_err("importing dependencies")
}

fn finish_import(
    result: miette::Result<()>,
    lockfile_path: &std::path::Path,
    lockfile_backup: &std::path::Path,
    lockfile_existed: bool,
) -> miette::Result<()> {
    match result {
        Ok(()) => {
            if lockfile_existed {
                std::fs::remove_file(lockfile_backup)
                    .into_diagnostic()
                    .wrap_err("removing the import lockfile backup")?;
            }
            Ok(())
        }
        Err(error) => {
            discard_failed_import(lockfile_path, lockfile_existed.then_some(lockfile_backup))
                .wrap_err_with(|| {
                    format!(
                        "restoring {} after the failed import: {error}",
                        lockfile_path.display(),
                    )
                })?;
            Err(error)
        }
    }
}
