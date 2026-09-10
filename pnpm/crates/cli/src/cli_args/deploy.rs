use crate::{
    State,
    cli_args::{
        install::{InstallArgs, NodeLinkerArg, resolve_bool_override},
        recursive::{AutoExcludeRoot, discover_workspace_projects, select_recursive_projects},
    },
};
use clap::Args;
use derive_more::{Display, Error};
use install::{legacy_deploy_preferred_versions, source_pnpmfile_hooks};
use lockfile::{
    ConvertCtx, DeployFiles, create_deploy_files, deployed_workspace_projects,
    load_deploy_lockfile, manifest_dependency_names,
};
use miette::{Context, Diagnostic, IntoDiagnostic};
use peers::{
    bind_singleton_peers, omit_peers_of_excluded_dependencies, prune_deploy_lockfile_graph,
};
use pnpm_config::{Config, NodeLinker, PackageImportMethod};
use pnpm_directory_fetcher::DirectoryFetcher;
use pnpm_fs::{lexical_normalize, remove_dirent};
use pnpm_lockfile::{
    DirectoryResolution, ImporterDepVersion, LazyLockfile, Lockfile, LockfileResolution,
    PackageKey, PackageMetadata, PkgName, PkgNameVerPeer, ProjectSnapshot, ResolvedDependencyMap,
    ResolvedDependencySpec, SnapshotDepRef, SnapshotEntry, TarballResolution, VersionPart,
    WantedLockfileSelection,
};
use pnpm_lockfile_preferred_versions::get_preferred_versions_from_lockfile_and_manifests;
use pnpm_package_manager::{
    ImportIndexedDirOpts, Install, apply_deploy_manifest_hook, import_indexed_dir,
};
use pnpm_package_manifest::{DependencyGroup, PackageManifest};
use pnpm_reporter::{LogEvent, LogLevel, PnpmLog, Reporter};
use pnpm_resolving_resolver_base::PreferredVersions;
use pnpm_workspace::{Project, WORKSPACE_MANIFEST_FILENAME, importer_id_from_root_dir};
use resolution::{
    ResolveBases, convert_package_key, convert_package_metadata, convert_resolved_dependency_spec,
    convert_snapshot, create_file_url_key, project_snapshot_to_snapshot_entry,
    validate_lockfile_local_path,
};
use serde_json::{Map, Value};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    fs, io,
    io::Write,
    path::{Path, PathBuf},
    sync::{Arc, atomic::AtomicU8},
};
use target::{
    ProjectPathKey, apply_deploy_hook, copy_project, is_ancestor_path, is_child_path,
    prepare_deploy_dir, relative_path, resolve_target_dir, same_path, validate_deploy_target,
    write_deploy_files,
};

#[derive(Debug, Args)]
pub struct DeployArgs {
    #[clap(flatten)]
    pub install_args: InstallArgs,

    /// Use the legacy deploy implementation.
    #[clap(long)]
    pub legacy: bool,

    /// Target deploy directory.
    #[arg(value_name = "DIR")]
    pub target_dirs: Vec<PathBuf>,
}

#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
enum DeployError {
    #[display("A deploy is only possible from inside a workspace")]
    #[diagnostic(code(ERR_PNPM_CANNOT_DEPLOY))]
    CannotDeploy,

    #[display("A deploy is only possible from inside a workspace")]
    #[diagnostic(
        code(ERR_PNPM_CANNOT_DEPLOY),
        help(r#"Maybe you wanted to invoke "pnpm run deploy""#)
    )]
    CannotDeployScript,

    #[display("No project was selected for deployment")]
    #[diagnostic(code(ERR_PNPM_NOTHING_TO_DEPLOY))]
    NothingToDeploy,

    #[display("Cannot deploy more than 1 project")]
    #[diagnostic(code(ERR_PNPM_CANNOT_DEPLOY_MANY))]
    CannotDeployMany,

    #[display("This command requires one parameter")]
    #[diagnostic(code(ERR_PNPM_INVALID_DEPLOY_TARGET))]
    InvalidDeployTarget,

    #[display("Deploy path {} is not empty", deploy_dir.display())]
    #[diagnostic(code(ERR_PNPM_DEPLOY_DIR_NOT_EMPTY))]
    DeployDirNotEmpty { deploy_dir: PathBuf },

    #[display("Refusing to deploy to unsafe target {}: {reason}", deploy_dir.display())]
    #[diagnostic(code(ERR_PNPM_INVALID_DEPLOY_TARGET))]
    UnsafeDeployTarget { deploy_dir: PathBuf, reason: &'static str },

    #[display(
        r#"Workspace package '{package}' declares a peer dependency on '{peer}', which resolves to more than one version ({versions}) in the deployed graph. Without "injectWorkspacePackages" there is no snapshot to bind it to."#
    )]
    #[diagnostic(
        code(ERR_PNPM_DEPLOY_AMBIGUOUS_PEER),
        help(
            r#"Pin '{peer}' to a single version with an "overrides" entry, set "injectWorkspacePackages" to true, or run "pnpm deploy" with the "--legacy" flag."#
        )
    )]
    AmbiguousPeer { package: String, peer: String, versions: String },

    #[display("The selected project is missing from pnpm-lock.yaml: {project_id}")]
    #[diagnostic(code(ERR_PNPM_CANNOT_DEPLOY))]
    MissingImporter { project_id: String },

    #[display(
        "Refusing to deploy unsafe lockfile path {}: path resolves outside workspace {}",
        path.display(),
        workspace_dir.display()
    )]
    #[diagnostic(code(ERR_PNPM_CANNOT_DEPLOY))]
    UnsafeLockfilePath { path: PathBuf, workspace_dir: PathBuf },
}

#[derive(Clone)]
struct ProjectInfo {
    name: Option<String>,
    peer_dependencies: Vec<PkgName>,
    /// Names the project declares as prod or optional dependencies. A peer it
    /// depends on itself is already bound by that edge, whether or not the
    /// deploy's group filter kept the edge in the deployed snapshot.
    declared_dependencies: HashSet<PkgName>,
}

struct SelectedProject {
    project: Project,
    projects_by_path: HashMap<ProjectPathKey, ProjectInfo>,
}

struct DeployWorkspaceConfig {
    patched_dependencies: Option<indexmap::IndexMap<String, String>>,
    allow_builds: HashMap<String, bool>,
}

enum DeployInstallMode {
    Legacy,
    Shared { workspace_config: DeployWorkspaceConfig },
}

impl DeployArgs {
    pub async fn run<ReporterT: Reporter + 'static>(
        self,
        config: &'static Config,
        dir: &Path,
    ) -> miette::Result<()> {
        let workspace_dir =
            config.workspace_dir.as_deref().ok_or_else(|| cannot_deploy_error(dir))?;
        let selected = select_project(config, workspace_dir, dir)?;
        if self.target_dirs.len() != 1 {
            return Err(DeployError::InvalidDeployTarget.into());
        }

        // Resolved before the target is prepared: a `pnpmfile` setting
        // naming a file that is not there fails the deploy, and it should
        // do so without having emptied the target first.
        let source_hooks = source_pnpmfile_hooks(config, &selected.project.root_dir)?;

        let force_legacy = self.legacy || config.force_legacy_deploy;
        let deploy_dir = resolve_target_dir(dir, &self.target_dirs[0]);
        self.prepare_target::<ReporterT>(config, workspace_dir, &selected, &deploy_dir, dir)?;

        if config.shares_one_lockfile() && !force_legacy {
            match Box::pin(self.deploy_from_shared_lockfile::<ReporterT>(
                config,
                workspace_dir,
                &selected,
                &deploy_dir,
                source_hooks.as_ref().map(Arc::clone),
            ))
            .await?
            {
                SharedDeployOutcome::Deployed => return Ok(()),
                SharedDeployOutcome::Fallback(warning) => warn::<ReporterT>(&deploy_dir, warning),
            }
        } else if config.shares_one_lockfile() && force_legacy {
            warn::<ReporterT>(
                &deploy_dir,
                "Shared workspace lockfile detected but configuration forces legacy deploy implementation.",
            );
        }

        self.run_legacy_deploy::<ReporterT>(config, &selected, &deploy_dir, source_hooks).await
    }

    async fn run_legacy_deploy<ReporterT: Reporter + 'static>(
        &self,
        config: &'static Config,
        selected: &SelectedProject,
        deploy_dir: &Path,
        source_hooks: Option<Arc<dyn pnpm_hooks::PnpmfileHooks>>,
    ) -> miette::Result<()> {
        apply_deploy_hook(&deploy_dir.join("package.json"))?;
        let preferred_versions_override = legacy_deploy_preferred_versions::<ReporterT>(
            config,
            config.lockfile_dir_for(&selected.project.root_dir),
        );
        // Boxed: the install future exceeds clippy's large-future threshold
        // (the captured `Config` is large).
        Box::pin(self.run_install_in_deploy_dir::<ReporterT>(
            config,
            deploy_dir,
            DeployInstallMode::Legacy,
            false,
            source_hooks,
            preferred_versions_override,
        ))
        .await
    }

    fn prepare_target<ReporterT: Reporter>(
        &self,
        config: &Config,
        workspace_dir: &Path,
        selected: &SelectedProject,
        deploy_dir: &Path,
        dir: &Path,
    ) -> miette::Result<()> {
        validate_deploy_target(
            deploy_dir,
            workspace_dir,
            &selected.project.root_dir,
            dir,
            self.install_args.force,
        )?;
        prepare_deploy_dir::<ReporterT>(workspace_dir, deploy_dir, self.install_args.force)?;
        copy_project::<ReporterT>(
            &selected.project.root_dir,
            deploy_dir,
            !config.deploy_all_files,
        )?;

        Ok(())
    }

    async fn deploy_from_shared_lockfile<ReporterT: Reporter + 'static>(
        &self,
        config: &'static Config,
        workspace_dir: &Path,
        selected: &SelectedProject,
        deploy_dir: &Path,
        source_hooks: Option<Arc<dyn pnpm_hooks::PnpmfileHooks>>,
    ) -> miette::Result<SharedDeployOutcome> {
        // The shared lockfile, and the importer ids naming the projects in
        // it, belong to the lockfile dir — which `lockfileDir` can move
        // away from the workspace this deploy selected its project from.
        let lockfile_dir = config.lockfile_dir_for(workspace_dir);
        let lockfile = match load_deploy_lockfile(workspace_dir, lockfile_dir)? {
            Ok(lockfile) => lockfile,
            Err(warning) => return Ok(SharedDeployOutcome::Fallback(warning)),
        };

        let project_id = importer_id_from_root_dir(lockfile_dir, &selected.project.root_dir);
        let dependency_groups = self
            .install_args
            .dependency_options
            .dependency_groups(config.optional)
            .collect::<Vec<_>>();
        let deploy_files = create_deploy_files(
            &lockfile,
            selected,
            &project_id,
            lockfile_dir,
            deploy_dir,
            config,
            &dependency_groups,
        )?;
        write_deploy_files(deploy_dir, &deploy_files)?;
        // Boxed for the same large-future reason as the legacy path above.
        Box::pin(self.run_install_in_deploy_dir::<ReporterT>(
            config,
            deploy_dir,
            DeployInstallMode::Shared { workspace_config: deploy_files.workspace_config },
            true,
            source_hooks,
            None,
        ))
        .await?;
        Ok(SharedDeployOutcome::Deployed)
    }
}

enum SharedDeployOutcome {
    Deployed,
    Fallback(String),
}

fn cannot_deploy_error(dir: &Path) -> miette::Report {
    let has_deploy_script = PackageManifest::from_path(dir.join("package.json"))
        .is_ok_and(|manifest| manifest.script("deploy", false).is_ok());
    if has_deploy_script {
        DeployError::CannotDeployScript.into()
    } else {
        DeployError::CannotDeploy.into()
    }
}

/// Resolve `--filter` / `--filter-prod` (and `-w`) to the single project
/// to deploy, through the same selection every other filtered command
/// runs against `dir`.
fn select_project(
    config: &Config,
    workspace_dir: &Path,
    dir: &Path,
) -> miette::Result<SelectedProject> {
    let (projects, _patterns) = discover_workspace_projects(workspace_dir, config)?;
    let projects_by_path = index_projects(&projects);

    let selected_root = {
        let selection =
            select_recursive_projects(&projects, config, dir, AutoExcludeRoot::Disabled)?;
        let mut selected = selection.selected.keys();
        match (selected.next(), selected.next()) {
            (None, _) => return Err(DeployError::NothingToDeploy.into()),
            (Some(root), None) => lexical_normalize(root),
            (Some(_), Some(_)) => return Err(DeployError::CannotDeployMany.into()),
        }
    };
    let project = projects
        .into_iter()
        .find(|project| lexical_normalize(&project.root_dir) == selected_root)
        .ok_or(DeployError::NothingToDeploy)?;
    Ok(SelectedProject { project, projects_by_path })
}

/// Index the workspace projects by [`ProjectPathKey`]. When two roots compare
/// equal, the first discovered project wins.
fn index_projects(projects: &[Project]) -> HashMap<ProjectPathKey, ProjectInfo> {
    let mut projects_by_path = HashMap::with_capacity(projects.len());
    for project in projects {
        projects_by_path.entry(ProjectPathKey::new(&project.root_dir)).or_insert_with(|| {
            ProjectInfo {
                name: project
                    .manifest
                    .value()
                    .get("name")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                peer_dependencies: manifest_dependency_names(
                    &project.manifest,
                    &["peerDependencies"],
                ),
                declared_dependencies: manifest_dependency_names(
                    &project.manifest,
                    &["dependencies", "optionalDependencies"],
                )
                .into_iter()
                .collect(),
            }
        });
    }
    projects_by_path
}

fn warn<ReporterT: Reporter>(prefix: &Path, message: impl Into<String>) {
    ReporterT::emit(&LogEvent::Pnpm(PnpmLog {
        level: LogLevel::Warn,
        message: message.into(),
        prefix: prefix.to_string_lossy().into_owned(),
    }));
}

#[cfg(test)]
mod tests;

mod target;

mod resolution;

mod peers;

mod lockfile;

mod install;
