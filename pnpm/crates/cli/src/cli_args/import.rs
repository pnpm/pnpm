mod yarn_patches;

use std::{collections::BTreeMap, path::Path};

use crate::State;
use clap::Args;
use miette::{Context, IntoDiagnostic};
use pnpm_lockfile::{EnvLockfile, Lockfile};
use pnpm_lockfile_import::{
    YARN_LOCKFILE_NAME, read_foreign_lockfile_versions, to_preferred_versions,
};
use pnpm_network::redact_url_for_display;
use pnpm_package_manager::{Install, PreferredVersionsOverride, ProjectMutation};
use pnpm_package_manifest::DependencyGroup;
use pnpm_reporter::Reporter;
use pnpm_resolving_resolver_base::PreferredVersions;

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
        let manifest_path = state.manifest.path().to_path_buf();
        let dir = manifest_path.parent().expect("manifest path always has a parent dir");

        self.warn_ignored_pnpr_server::<Reporter>(state.config);

        let mut preferred_versions = PreferredVersionsOverride::from(to_preferred_versions(
            &read_foreign_lockfile_versions(dir)?,
        ));
        preferred_versions.by_importer = nested_yarn_lock_preferred_versions(&state, dir)?;
        let state = yarn_patches::import_yarn_patches::<Reporter>(state, dir)?;

        let lockfile_dir = state.lockfile_dir();
        let lockfile_path = state.lockfile_path();
        let env_lockfile = if state.config.wanted_lockfile_name() == Lockfile::FILE_NAME {
            EnvLockfile::read(lockfile_dir)
                .into_diagnostic()
                .wrap_err("reading the env lockfile before import")?
        } else {
            None
        };

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

    fn warn_ignored_pnpr_server<Reporter: self::Reporter>(&self, config: &pnpm_config::Config) {
        if let Some(pnpr_server) =
            self.pnpr_server.as_deref().or(config.pnpr_server.as_deref())
        {
            let pnpr_server = redact_url_for_display(pnpr_server);
            pnpm_reporter::emit_global_warning::<Reporter>(&format!(
                r#""pnpm import" resolves dependencies locally, so the pnpr server at {pnpr_server} is not used"#,
            ));
        }
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

/// Pins from a `yarn.lock` in a workspace project other than the directory
/// passed to `pnpm import`. The root lockfile stays the shared seed.
fn nested_yarn_lock_preferred_versions(
    state: &State,
    imported_dir: &Path,
) -> miette::Result<BTreeMap<String, PreferredVersions>> {
    let Some(workspace_dir) = state.config.workspace_dir.as_deref() else {
        return Ok(BTreeMap::new());
    };
    let (projects, _) = super::recursive::discover_workspace_projects(workspace_dir, state.config)?;
    let lockfile_dir = state.lockfile_dir();
    let imported_id = pnpm_workspace::importer_id_from_root_dir(lockfile_dir, imported_dir);
    let mut by_importer = BTreeMap::new();
    for project in projects {
        let importer_id =
            pnpm_workspace::importer_id_from_root_dir(lockfile_dir, &project.root_dir);
        if importer_id == imported_id {
            continue;
        }
        if !project.root_dir.join(YARN_LOCKFILE_NAME).is_file() {
            continue;
        }
        let versions = read_foreign_lockfile_versions(&project.root_dir)?;
        by_importer.insert(importer_id, to_preferred_versions(&versions));
    }
    Ok(by_importer)
}

async fn import_versions<Reporter: self::Reporter + 'static>(
    state: &State,
    lockfile_path: &std::path::Path,
    preferred_versions: PreferredVersionsOverride,
) -> miette::Result<()> {
    let import_lockfile = pnpm_lockfile::LazyLockfile::preloaded(None);

    {
        let mut base_install = Install::new(
            std::sync::Arc::clone(&state.tarball_mem_cache),
            &state.resolved_packages,
            (&state.http_client, std::sync::Arc::clone(&state.http_client)),
            state.config,
            &state.manifest,
            pnpm_lockfile::MaybeLazyLockfile::Lazy(&import_lockfile),
            [DependencyGroup::Prod, DependencyGroup::Dev, DependencyGroup::Optional].into_iter(),
        );
        base_install.lockfile_policy.prefer_frozen = Some(false);
        base_install.lockfile_policy.trust = false;
        base_install.execution.mutation = ProjectMutation::NoInstall;
        base_install.execution.lockfile_only = true;
        base_install.resolution.update_seed_policy =
            pnpm_package_manager::UpdateSeedPolicy::drop_all();
        base_install.resolution.preferred_versions_override = Some(preferred_versions);
        base_install.context.lockfile_path = Some(lockfile_path);
        base_install
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
