mod yarn_patches;

use std::path::{
    Path,
    PathBuf,
};

use crate::State;
use clap::Args;
use miette::{
    Context,
    IntoDiagnostic,
};
use pnpm_lockfile::{
    EnvLockfile,
    Lockfile,
    SymlinkedLockfile,
    lockfile_write_target,
};
use pnpm_lockfile_import::{
    YARN_LOCKFILE_NAME,
    read_foreign_lockfile_versions,
    to_preferred_versions,
};
use pnpm_network::redact_url_for_display;
use pnpm_package_manager::{
    DiscoveredWorkspace,
    PreferredVersionsOverride,
    ProjectMutation,
};
use pnpm_package_manifest::DependencyGroup;
use pnpm_reporter::Reporter;
use pnpm_resolving_resolver_base::PreferredVersions;
use pnpm_workspace::Project;

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
        let dir = state.project_dir().to_path_buf();

        self.warn_ignored_pnpr_server::<Reporter>(state.config);

        let shared = to_preferred_versions(&read_foreign_lockfile_versions(&dir)?);
        let mut state = yarn_patches::import_yarn_patches::<Reporter>(state, &dir)?;
        let mut backups = Vec::new();
        let result = match state.config.workspace_dir.as_deref() {
            Some(workspace) => {
                import_workspace_projects::<Reporter>(&mut state, workspace, shared, &mut backups)
                    .await
            }
            None => replace_lockfile::<Reporter>(&state, shared.into(), &mut backups).await,
        };
        finish_import(result, backups)
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

async fn import_workspace_projects<Reporter: self::Reporter + 'static>(
    state: &mut State,
    workspace: &Path,
    shared: PreferredVersions,
    backups: &mut Vec<LockfileBackup>,
) -> miette::Result<()> {
    let (mut projects, _) = super::recursive::discover_workspace_projects(workspace, state.config)?;
    pnpm_package_manager::apply_runtime_on_fail_to_projects(state.config, &mut projects);
    check_workspace_cycles::<Reporter>(state.config, &projects, workspace)?;
    let discovered = DiscoveredWorkspace::new(projects);
    state.discovered_workspace = Some(discovered.clone());
    import_workspace::<Reporter>(state, &discovered, shared, backups).await
}

fn check_workspace_cycles<Reporter: self::Reporter>(
    config: &pnpm_config::Config,
    projects: &[Project],
    workspace: &Path,
) -> miette::Result<()> {
    if config.ignore_workspace_cycles {
        return Ok(());
    }
    let catalogs = super::catalogs::configured_catalogs(config)?;
    let cycles = pnpm_package_manager::install_scope_cycles(
        config,
        projects,
        None,
        pnpm_workspace_projects_graph::WorkspaceCatalogs {
            catalogs: &catalogs,
            workspace_dir: workspace,
        },
    )?;
    pnpm_package_manager::report_workspace_cycles::<Reporter>(
        config,
        workspace,
        cycles.as_deref(),
    )?;
    Ok(())
}

async fn import_workspace<Reporter: self::Reporter + 'static>(
    state: &State,
    workspace: &DiscoveredWorkspace,
    shared: PreferredVersions,
    backups: &mut Vec<LockfileBackup>,
) -> miette::Result<()> {
    let caches = crate::state::DedicatedCaches::default();
    let mut preferred_versions = PreferredVersionsOverride::from(shared);
    for project in workspace
        .projects()
        .iter()
        .filter(|project| project.root_dir != state.project_dir())
    {
        let project_dir = &project.root_dir;
        let nested = nested_yarn_preferences(project_dir)?;
        if state.config.shares_one_lockfile() {
            if let Some(nested) = nested {
                let id =
                    pnpm_workspace::importer_id_from_root_dir(state.lockfile_dir(), project_dir);
                preferred_versions.by_importer.insert(id, nested);
            }
        } else {
            let mut pins = PreferredVersionsOverride::from(preferred_versions.shared.clone());
            if let Some(nested) = nested {
                pins.by_importer.insert(".".to_string(), nested);
            }
            let child = super::pipelines::init_dedicated_project_state(
                state.config,
                project_dir,
                super::pipelines::project_manifest_name(project),
                false,
                Some(std::sync::Arc::clone(&state.http_client)),
            )?
            .into_dedicated_project(&caches, None, Some(workspace.clone()));
            replace_lockfile::<Reporter>(&child, pins, backups).await?;
        }
    }
    replace_lockfile::<Reporter>(state, preferred_versions, backups).await
}

fn nested_yarn_preferences(dir: &Path) -> miette::Result<Option<PreferredVersions>> {
    if !dir.join(YARN_LOCKFILE_NAME).is_file() {
        return Ok(None);
    }
    Ok(Some(to_preferred_versions(&read_foreign_lockfile_versions(dir)?)))
}

async fn replace_lockfile<Reporter: self::Reporter + 'static>(
    state: &State,
    preferred_versions: PreferredVersionsOverride,
    backups: &mut Vec<LockfileBackup>,
) -> miette::Result<()> {
    let lockfile_dir = state.lockfile_dir();
    let symlinked = state.config.symlinked_lockfile();
    let lockfile_path =
        lockfile_write_target(&state.lockfile_path(), symlinked).into_diagnostic()?.into_owned();
    let env_lockfile = if state.config.wanted_lockfile_name() == Lockfile::FILE_NAME {
        EnvLockfile::read(lockfile_dir)
            .into_diagnostic()
            .wrap_err("reading the env lockfile before import")?
    } else {
        None
    };

    backups.push(LockfileBackup::create(lockfile_path.clone(), symlinked, backups.len())?);

    import_versions::<Reporter>(state, &lockfile_path, preferred_versions).await?;

    if let Some(env_lockfile) = env_lockfile {
        env_lockfile
            .write(
                lockfile_dir,
                symlinked,
                pnpm_lockfile::EnvDocumentFormat::for_pnpm(pnpm_config::PNPM_VERSION),
            )
            .into_diagnostic()
            .wrap_err("preserving the env lockfile after import")?;
    }
    Ok(())
}

fn back_up_lockfile(
    lockfile_path: &Path,
    backup_path: &Path,
    symlinked: SymlinkedLockfile<'_>,
) -> miette::Result<()> {
    match symlinked {
        // Copied, so the link still resolves for the writers that save
        // through it during the import.
        SymlinkedLockfile::WriteThrough { .. } => pnpm_fs::copy_permissions(lockfile_path)
            .and_then(|permissions| {
                pnpm_fs::copy_file_exclusive(lockfile_path, backup_path, &permissions, |_| Ok(()))
            }),
        SymlinkedLockfile::Refuse => std::fs::rename(lockfile_path, backup_path),
    }
    .into_diagnostic()
    .wrap_err("backing up existing pnpm-lock.yaml")
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
    preferred_versions: PreferredVersionsOverride,
) -> miette::Result<()> {
    let import_lockfile = pnpm_lockfile::LazyLockfile::preloaded(None);

    {
        let mut base_install =
            state.install([DependencyGroup::Prod, DependencyGroup::Dev, DependencyGroup::Optional]);
        base_install.context.lockfile = pnpm_lockfile::MaybeLazyLockfile::Lazy(&import_lockfile);
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

struct LockfileBackup {
    path: PathBuf,
    original: Option<PathBuf>,
}

impl LockfileBackup {
    fn create(
        path: PathBuf,
        symlinked: SymlinkedLockfile<'_>,
        index: usize,
    ) -> miette::Result<Self> {
        let original = if path.try_exists().into_diagnostic()? {
            let backup =
                path.with_extension(format!("yaml.{}.{index}.import.bak", pnpm_fs::process_id()));
            back_up_lockfile(&path, &backup, symlinked)?;
            Some(backup)
        } else {
            None
        };
        Ok(Self { path, original })
    }
}

fn finish_import(result: miette::Result<()>, backups: Vec<LockfileBackup>) -> miette::Result<()> {
    let mut result = result;
    let restore = result.is_err();
    for backup in backups.into_iter().rev() {
        let cleanup = if restore {
            discard_failed_import(&backup.path, backup.original.as_deref())
        } else if let Some(path) = backup.original {
            std::fs::remove_file(path).into_diagnostic()
        } else {
            continue;
        };
        if let Err(error) = cleanup {
            let message = format!("finishing import of {}: {error}", backup.path.display());
            result = match result {
                Ok(()) => Err(error.wrap_err(message)),
                Err(previous) => Err(previous.wrap_err(message)),
            };
        }
    }
    result
}
