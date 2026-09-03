use crate::{
    State,
    cli_args::{
        install::{InstallArgs, NodeLinkerArg, resolve_bool_override},
        recursive::{AutoExcludeRoot, discover_workspace_projects, select_recursive_projects},
    },
};
use clap::Args;
use derive_more::{Display, Error};
use miette::{Context, Diagnostic, IntoDiagnostic};
use pnpm_config::{Config, NodeLinker, PackageImportMethod};
use pnpm_directory_fetcher::DirectoryFetcher;
use pnpm_fs::{lexical_normalize, remove_dirent};
use pnpm_lockfile::{
    DirectoryResolution, ImporterDepVersion, LazyLockfile, Lockfile, LockfileResolution,
    MaybeLazyLockfile, PackageKey, PackageMetadata, PkgName, PkgNameVerPeer, ProjectSnapshot,
    ResolvedDependencyMap, ResolvedDependencySpec, SnapshotDepRef, SnapshotEntry,
    TarballResolution, VersionPart, WantedLockfileSelection,
};
use pnpm_package_manager::{
    ImportIndexedDirOpts, Install, ProjectMutation, UpdateSeedPolicy, apply_deploy_manifest_hook,
    import_indexed_dir,
};
use pnpm_package_manifest::{DependencyGroup, PackageManifest};
use pnpm_reporter::{LogEvent, LogLevel, PnpmLog, Reporter};
use pnpm_workspace::{Project, WORKSPACE_MANIFEST_FILENAME, importer_id_from_root_dir};
use serde_json::{Map, Value};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{Arc, atomic::AtomicU8},
};

#[cfg(windows)]
use std::os::windows::fs::MetadataExt;

#[cfg(windows)]
const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;

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
    root_dir: PathBuf,
    name: Option<String>,
    peer_dependencies: Vec<PkgName>,
    /// Names the project declares as prod or optional dependencies. A peer it
    /// depends on itself is already bound by that edge, whether or not the
    /// deploy's group filter kept the edge in the deployed snapshot.
    declared_dependencies: HashSet<PkgName>,
}

struct SelectedProject {
    project: Project,
    all_projects: Vec<ProjectInfo>,
}

struct DeployWorkspaceConfig {
    patched_dependencies: Option<indexmap::IndexMap<String, String>>,
    allow_builds: HashMap<String, bool>,
}

struct DeployFiles {
    manifest: Value,
    lockfile: Lockfile,
    workspace_manifest: Option<Value>,
    workspace_config: DeployWorkspaceConfig,
}

enum DeployInstallMode {
    Legacy,
    Shared { workspace_config: DeployWorkspaceConfig },
}

struct ConvertCtx<'a> {
    all_projects: &'a [ProjectInfo],
    deploy_dir: &'a Path,
    lockfile_dir: &'a Path,
    deployed_project_root: &'a Path,
}

struct ResolveBases<'a> {
    file_base: &'a Path,
    link_base: &'a Path,
}

struct LocalResolve {
    resolved_path: PathBuf,
    suffix: String,
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

        let force_legacy = self.legacy || config.force_legacy_deploy;
        let deploy_dir = resolve_target_dir(dir, &self.target_dirs[0]);
        // Deploy's `--force` (declared on the flattened `InstallArgs`)
        // does double duty: besides the install-side force semantics it
        // also deletes a non-empty deploy path.
        validate_deploy_target(
            &deploy_dir,
            workspace_dir,
            &selected.project.root_dir,
            dir,
            self.install_args.force,
        )?;
        prepare_deploy_dir::<ReporterT>(workspace_dir, &deploy_dir, self.install_args.force)?;
        copy_project::<ReporterT>(
            &selected.project.root_dir,
            &deploy_dir,
            !config.deploy_all_files,
        )?;

        if config.shares_one_lockfile() && !force_legacy {
            match Box::pin(self.deploy_from_shared_lockfile::<ReporterT>(
                config,
                workspace_dir,
                &selected,
                &deploy_dir,
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

        apply_deploy_hook(&deploy_dir.join("package.json"))?;
        // Boxed: the install future exceeds clippy's large-future threshold
        // (the captured `Config` is large).
        Box::pin(self.run_install_in_deploy_dir::<ReporterT>(
            config,
            &deploy_dir,
            DeployInstallMode::Legacy,
            false,
        ))
        .await
    }

    async fn deploy_from_shared_lockfile<ReporterT: Reporter + 'static>(
        &self,
        config: &'static Config,
        workspace_dir: &Path,
        selected: &SelectedProject,
        deploy_dir: &Path,
    ) -> miette::Result<SharedDeployOutcome> {
        // The shared lockfile, and the importer ids naming the projects in
        // it, belong to the lockfile dir — which `lockfileDir` can move
        // away from the workspace this deploy selected its project from.
        let lockfile_dir = config.lockfile_dir_for(workspace_dir);
        // Every path this deploy resolves is a lockfile-relative importer
        // id joined onto that dir, and none of them may escape it. A pin
        // that does not contain the workspace makes each project's id
        // climb out (`../packages/app`), so the shared path cannot
        // describe this layout at all: hand it to the legacy installer,
        // which resolves the deployed manifest on its own.
        if !same_path(workspace_dir, lockfile_dir) && !is_ancestor_path(lockfile_dir, workspace_dir)
        {
            return Ok(SharedDeployOutcome::Fallback(format!(
                "The lockfile at {} does not contain the workspace, so its importer paths cannot be deployed. Falling back to installing without it.",
                lockfile_dir.display(),
            )));
        }
        let Some(lockfile) = Lockfile::load_wanted_from_dir(lockfile_dir)
            .map_err(miette::Report::new)
            .wrap_err("read shared lockfile")?
        else {
            return Ok(SharedDeployOutcome::Fallback(
                "Shared lockfile not found. Falling back to installing without a lockfile."
                    .to_string(),
            ));
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
        ))
        .await?;
        Ok(SharedDeployOutcome::Deployed)
    }

    async fn run_install_in_deploy_dir<ReporterT: Reporter + 'static>(
        &self,
        base_config: &Config,
        deploy_dir: &Path,
        mode: DeployInstallMode,
        frozen_lockfile: bool,
    ) -> miette::Result<()> {
        let node_linker = self
            .install_args
            .node_linker
            .map_or(base_config.node_linker, NodeLinkerArg::into_config);
        let mut deploy_config = create_deploy_install_config(base_config, deploy_dir, node_linker);
        deploy_config.prefer_frozen_lockfile = frozen_lockfile;
        // pnpm's deploy forwards `--force` into the install, where it
        // bypasses the installability check so optional dependencies of
        // every platform are materialized (see `Config::force`).
        deploy_config.force = self.install_args.force;

        let legacy = matches!(&mode, DeployInstallMode::Legacy);
        match mode {
            DeployInstallMode::Legacy => {}
            DeployInstallMode::Shared { workspace_config } => {
                deploy_config.workspace_dir = deploy_dir.to_path_buf().into();
                deploy_config.inject_workspace_packages = false;
                deploy_config.overrides = None;
                deploy_config.package_extensions = None;
                deploy_config.config_dependencies = None;
                deploy_config.patched_dependencies = workspace_config.patched_dependencies;
                deploy_config.allow_builds = workspace_config.allow_builds;
            }
        }

        let deploy_config = Config::leak(deploy_config);
        let mut state =
            State::init(deploy_dir.join("package.json"), deploy_config, frozen_lockfile)
                .wrap_err("initialize the deploy install state")?;
        if legacy {
            // The deployed project is not one of the source workspace's
            // importers — the deploy hook rewrites the copied manifest —
            // so its resolution must not be seeded from the workspace
            // lockfile. Plain `pnpm-lock.yaml` whatever the branch settings
            // say, for the same reason: they describe that workspace
            // resolution, and pnpm's deploy reads and writes the deployed
            // lockfile under the plain name too.
            state.lockfile = if state.config.lockfile || frozen_lockfile {
                LazyLockfile::deferred(deploy_dir.to_path_buf(), WantedLockfileSelection::default())
            } else {
                LazyLockfile::disabled()
            };
        }
        let State { tarball_mem_cache, http_client, config, manifest, lockfile, resolved_packages } =
            &state;
        // Deploying the workspace root copies `pnpm-workspace.yaml` and
        // the projects it globs, none of which the generated frozen
        // lockfile describes. pnpm installs the deploy directory with no
        // workspace at all; pacquet's equivalent is a workspace holding
        // the deployed project alone.
        let workspace_projects_override = (!legacy).then(|| {
            vec![Project {
                root_dir: deploy_dir.to_path_buf(),
                manifest: manifest.clone(),
                dependency_manifest: None,
            }]
        });

        let supported_architectures = self
            .install_args
            .supported_architectures
            .apply_to(config.supported_architectures.clone());
        let skip_runtimes = config.skip_runtimes || self.install_args.no_runtime;
        let trust_lockfile = resolve_bool_override(
            self.install_args.trust_lockfile,
            self.install_args.no_trust_lockfile,
            config.trust_lockfile,
        );
        let lockfile_path = config.lockfile.then(|| deploy_dir.join(Lockfile::FILE_NAME));
        let prefer_frozen_lockfile = frozen_lockfile.then_some(true).or(Some(false));
        let dependency_groups = self
            .install_args
            .dependency_options
            .dependency_groups(config.optional)
            .collect::<Vec<_>>();

        let install = Install {
            tarball_mem_cache: Arc::clone(tarball_mem_cache),
            http_client,
            http_client_arc: Arc::clone(http_client),
            config,
            manifest,
            emit_initial_manifest: true,
            lockfile: MaybeLazyLockfile::Lazy(lockfile),
            lockfile_path: lockfile_path.as_deref(),
            dependency_groups,
            frozen_lockfile,
            prefer_frozen_lockfile,
            ignore_manifest_check: false,
            skip_runtimes,
            trust_lockfile,
            update_checksums: false,
            mutation: ProjectMutation::InstallWorkspace,
            installs_only: true,
            resolved_packages,
            supported_architectures,
            node_linker,
            lockfile_only: false,
            dry_run: false,
            persist_policy_excludes: false,
            update_seed_policy: UpdateSeedPolicy::KeepAll,
            preferred_versions_override: None,
            auth_override: None,
            resolution_observer: None,
            peer_issues_sink: None,
            deps_requiring_build_sink: None,
            catalogs_override: None,
            disable_optimistic_repeat_install: true,
            pnpmfile_hook_override: None,
            workspace_projects_override,
        };
        if legacy {
            install.run_legacy_deploy::<ReporterT>().await
        } else {
            install.run::<ReporterT>().await
        }
        .wrap_err("installing deployed dependencies")
    }
}

fn create_deploy_install_config(
    base_config: &Config,
    deploy_dir: &Path,
    node_linker: NodeLinker,
) -> Config {
    let mut deploy_config = base_config.clone();
    deploy_config.modules_dir = deploy_dir.join("node_modules");
    deploy_config.virtual_store_dir = deploy_dir.join("node_modules/.pnpm");
    // The deploy directory owns the lockfile this install runs against —
    // the generated one for a shared deploy, its own resolution for the
    // legacy path. A `lockfileDir` pinning the *source* workspace's
    // lockfile must not redirect either.
    deploy_config.lockfile_dir = None;
    deploy_config.global_virtual_store_dir = deploy_config.virtual_store_dir.clone();
    deploy_config.enable_global_virtual_store = false;
    deploy_config.pnpr_server = None;
    deploy_config.optimistic_repeat_install = false;
    deploy_config.dedupe_peer_dependents = false;
    deploy_config.dedupe_injected_deps = false;
    deploy_config.node_linker = node_linker;
    deploy_config
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
    let all_projects = projects
        .iter()
        .map(|project| ProjectInfo {
            root_dir: lexical_normalize(&project.root_dir),
            name: project.manifest.value().get("name").and_then(Value::as_str).map(str::to_string),
            peer_dependencies: manifest_dependency_names(&project.manifest, &["peerDependencies"]),
            declared_dependencies: manifest_dependency_names(
                &project.manifest,
                &["dependencies", "optionalDependencies"],
            )
            .into_iter()
            .collect(),
        })
        .collect::<Vec<_>>();

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
    Ok(SelectedProject { project, all_projects })
}

fn resolve_target_dir(dir: &Path, target: &Path) -> PathBuf {
    if target.is_absolute() {
        lexical_normalize(target)
    } else {
        lexical_normalize(&dir.join(target))
    }
}

fn validate_deploy_target(
    deploy_dir: &Path,
    workspace_dir: &Path,
    project_dir: &Path,
    dir: &Path,
    force: bool,
) -> miette::Result<()> {
    let deploy_dir = lexical_normalize(deploy_dir);
    let workspace_dir = lexical_normalize(workspace_dir);
    let project_dir = lexical_normalize(project_dir);
    let dir = lexical_normalize(dir);

    if same_path(&deploy_dir, &workspace_dir) {
        return unsafe_deploy_target(&deploy_dir, "target is the workspace root");
    }
    if is_ancestor_path(&deploy_dir, &workspace_dir) {
        return unsafe_deploy_target(&deploy_dir, "target contains the workspace root");
    }
    if same_path(&deploy_dir, &project_dir) {
        return unsafe_deploy_target(&deploy_dir, "target is the selected project root");
    }
    if is_ancestor_path(&deploy_dir, &project_dir) {
        return unsafe_deploy_target(&deploy_dir, "target contains the selected project");
    }
    if same_path(&deploy_dir, &dir) {
        return unsafe_deploy_target(&deploy_dir, "target is the current directory");
    }
    if is_ancestor_path(&deploy_dir, &dir) {
        return unsafe_deploy_target(&deploy_dir, "target contains the current directory");
    }
    if force && !is_child_path(&deploy_dir, &workspace_dir) {
        return unsafe_deploy_target(&deploy_dir, "target is outside the workspace");
    }
    if is_child_path(&deploy_dir, &workspace_dir) {
        validate_workspace_child_target_components(&workspace_dir, &deploy_dir)?;
    }

    Ok(())
}

fn validate_workspace_child_target_components(
    workspace_dir: &Path,
    deploy_dir: &Path,
) -> miette::Result<()> {
    let mut current = workspace_dir.to_path_buf();
    for component in relative_components_from_child(workspace_dir, deploy_dir)? {
        current.push(component);
        let metadata = match fs::symlink_metadata(&current) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => {
                return Err(error)
                    .into_diagnostic()
                    .wrap_err_with(|| format!("inspect deploy target {}", current.display()));
            }
        };
        if is_unsafe_deploy_link(&metadata) {
            return unsafe_deploy_target(&current, "target path contains a symlink or junction");
        }
    }
    Ok(())
}

fn unsafe_deploy_target<Output>(deploy_dir: &Path, reason: &'static str) -> miette::Result<Output> {
    Err(DeployError::UnsafeDeployTarget { deploy_dir: deploy_dir.to_path_buf(), reason }.into())
}

fn is_ancestor_path(parent: &Path, child: &Path) -> bool {
    is_child_path(child, parent)
}

fn is_child_path(child: &Path, parent: &Path) -> bool {
    has_path_prefix(child, parent) && !same_path(child, parent)
}

fn prepare_deploy_dir<ReporterT: Reporter>(
    workspace_dir: &Path,
    deploy_dir: &Path,
    force: bool,
) -> miette::Result<()> {
    let workspace_dir = lexical_normalize(workspace_dir);
    let deploy_dir = lexical_normalize(deploy_dir);
    let workspace_child_target = is_child_path(&deploy_dir, &workspace_dir);
    if workspace_child_target {
        create_workspace_child_target_parents(&workspace_dir, &deploy_dir)?;
    }
    if !is_empty_dir_or_absent(&deploy_dir)? {
        if !force {
            return Err(DeployError::DeployDirNotEmpty { deploy_dir }.into());
        }
        warn::<ReporterT>(
            &deploy_dir,
            format!("using --force, deleting deploy path {}", deploy_dir.display()),
        );
    }
    if workspace_child_target {
        validate_workspace_child_target_components(&workspace_dir, &deploy_dir)?;
    }
    remove_path_if_exists(&deploy_dir)?;
    if workspace_child_target {
        create_workspace_child_target_parents(&workspace_dir, &deploy_dir)?;
        create_workspace_child_target_dir(&workspace_dir, &deploy_dir)
    } else {
        fs::create_dir_all(&deploy_dir)
            .into_diagnostic()
            .wrap_err_with(|| format!("create deploy directory {}", deploy_dir.display()))
    }
}

fn create_workspace_child_target_parents(
    workspace_dir: &Path,
    deploy_dir: &Path,
) -> miette::Result<()> {
    let Some(parent) = deploy_dir.parent() else {
        return Ok(());
    };
    let mut current = workspace_dir.to_path_buf();
    for component in relative_components_from_child(workspace_dir, parent)? {
        current.push(component);
        create_workspace_child_target_component(&current)?;
    }
    Ok(())
}

fn create_workspace_child_target_dir(
    workspace_dir: &Path,
    deploy_dir: &Path,
) -> miette::Result<()> {
    match fs::create_dir(deploy_dir) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            return unsafe_deploy_target(deploy_dir, "target changed during deploy preparation");
        }
        Err(error) => {
            return Err(error)
                .into_diagnostic()
                .wrap_err_with(|| format!("create deploy directory {}", deploy_dir.display()));
        }
    }
    validate_workspace_child_target_components(workspace_dir, deploy_dir)
}

fn create_workspace_child_target_component(component: &Path) -> miette::Result<()> {
    match fs::create_dir(component) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(error) => {
            return Err(error).into_diagnostic().wrap_err_with(|| {
                format!("create deploy target component {}", component.display())
            });
        }
    }
    let metadata = fs::symlink_metadata(component)
        .into_diagnostic()
        .wrap_err_with(|| format!("inspect deploy target {}", component.display()))?;
    if is_unsafe_deploy_link(&metadata) {
        return unsafe_deploy_target(component, "target path contains a symlink or junction");
    }
    if !metadata.is_dir() {
        return unsafe_deploy_target(component, "target path contains a non-directory");
    }
    Ok(())
}

fn is_empty_dir_or_absent(path: &Path) -> miette::Result<bool> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(true),
        Err(error) => return Err(error).into_diagnostic(),
    };
    if !metadata.is_dir() || is_unsafe_deploy_link(&metadata) {
        return Ok(false);
    }
    let mut entries = fs::read_dir(path).into_diagnostic()?;
    Ok(entries.next().is_none())
}

fn remove_path_if_exists(path: &Path) -> miette::Result<()> {
    match remove_dirent(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error)
            .into_diagnostic()
            .wrap_err_with(|| format!("remove deploy path {}", path.display())),
    }
}

fn is_unsafe_deploy_link(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        metadata.file_type().is_symlink()
            || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    }
    #[cfg(not(windows))]
    {
        metadata.file_type().is_symlink()
    }
}

fn copy_project<ReporterT: Reporter>(
    src: &Path,
    dest: &Path,
    include_only_package_files: bool,
) -> miette::Result<()> {
    let output = DirectoryFetcher {
        directory: src.to_path_buf(),
        include_only_package_files,
        resolve_symlinks: false,
        allow_path_escape: false,
    }
    .run()
    .map_err(miette::Report::new)
    .wrap_err("fetch project files")?;
    let logged_methods = AtomicU8::new(0);
    import_indexed_dir::<ReporterT>(
        &logged_methods,
        PackageImportMethod::CloneOrCopy,
        dest,
        &output.files_map,
        ImportIndexedDirOpts { force: true, ..ImportIndexedDirOpts::default() },
    )
    .map_err(miette::Report::new)
    .wrap_err("copy project files")
}

fn apply_deploy_hook(manifest_path: &Path) -> miette::Result<()> {
    let mut manifest = PackageManifest::from_path(manifest_path.to_path_buf())
        .wrap_err("read deployed manifest")?;
    apply_deploy_manifest_hook(manifest.value_mut());
    manifest.save().wrap_err("write deployed manifest")
}

fn manifest_dependency_names(manifest: &PackageManifest, groups: &[&str]) -> Vec<PkgName> {
    groups
        .iter()
        .filter_map(|group| manifest.value().get(group))
        .filter_map(Value::as_object)
        .flat_map(|dependencies| dependencies.keys())
        .filter_map(|name| name.parse().ok())
        .collect()
}

fn create_deploy_files(
    lockfile: &Lockfile,
    selected: &SelectedProject,
    project_id: &str,
    lockfile_dir: &Path,
    deploy_dir: &Path,
    config: &Config,
    dependency_groups: &[DependencyGroup],
) -> miette::Result<DeployFiles> {
    let input_snapshot = lockfile
        .importers
        .get(project_id)
        .ok_or_else(|| DeployError::MissingImporter { project_id: project_id.to_string() })?;
    let deployed_project_root =
        validate_lockfile_local_path(&lockfile_dir.join(project_id), lockfile_dir)?;
    let ctx = ConvertCtx {
        all_projects: &selected.all_projects,
        deploy_dir,
        lockfile_dir,
        deployed_project_root: &deployed_project_root,
    };
    let mut target_snapshot = input_snapshot.clone();
    target_snapshot.specifiers = Some(HashMap::new());
    target_snapshot.dependencies = Some(HashMap::new());
    target_snapshot.dev_dependencies = Some(HashMap::new());
    target_snapshot.optional_dependencies = Some(HashMap::new());
    let declared_dependencies = selected
        .project
        .manifest
        .available_dependency_names(None)
        .into_iter()
        .collect::<HashSet<_>>();
    let peer_only_dependencies = selected
        .project
        .manifest
        .dependencies([DependencyGroup::Peer])
        .map(|(name, _)| name.to_string())
        .filter(|name| !declared_dependencies.contains(name))
        .collect::<HashSet<_>>();

    let selected_root = lexical_normalize(&selected.project.root_dir);
    let selected_bases = ResolveBases { file_base: lockfile_dir, link_base: &selected_root };
    // An excluded group's direct dependencies are left out of both the
    // deployed manifest and the deployed importer, because the graph prune
    // below drops the packages they would point at.
    let include_prod = dependency_groups.contains(&DependencyGroup::Prod);
    fill_target_dependency_map(
        &mut target_snapshot.dependencies,
        input_snapshot
            .dependencies
            .iter()
            .flatten()
            .filter(|(name, _)| include_prod || peer_only_dependencies.contains(&name.to_string())),
        &ctx,
        &selected_bases,
    )?;
    let include_dev = dependency_groups.contains(&DependencyGroup::Dev);
    fill_target_dependency_map(
        &mut target_snapshot.dev_dependencies,
        input_snapshot
            .dev_dependencies
            .iter()
            .flatten()
            .filter(|(name, _)| include_dev || peer_only_dependencies.contains(&name.to_string())),
        &ctx,
        &selected_bases,
    )?;
    let include_optional = dependency_groups.contains(&DependencyGroup::Optional);
    fill_target_dependency_map(
        &mut target_snapshot.optional_dependencies,
        input_snapshot.optional_dependencies.iter().flatten().filter(|(name, _)| {
            include_optional || peer_only_dependencies.contains(&name.to_string())
        }),
        &ctx,
        &selected_bases,
    )?;
    drop_empty_dependency_map(&mut target_snapshot.dependencies);
    drop_empty_dependency_map(&mut target_snapshot.dev_dependencies);
    drop_empty_dependency_map(&mut target_snapshot.optional_dependencies);

    let mut packages = HashMap::new();
    if let Some(input_packages) = lockfile.packages.as_ref() {
        for (key, metadata) in input_packages {
            let output_key = convert_package_key(key, &ctx)?;
            packages.insert(output_key, convert_package_metadata(metadata, &ctx)?);
        }
    }
    for importer_path in lockfile.importers.keys() {
        if importer_path == project_id {
            continue;
        }
        let project_root =
            validate_lockfile_local_path(&lockfile_dir.join(importer_path), lockfile_dir)?;
        let package_key = create_file_url_key(&project_root, "", &selected.all_projects, None)?;
        packages.insert(
            package_key,
            PackageMetadata {
                resolution: LockfileResolution::Directory(DirectoryResolution {
                    directory: relative_path(deploy_dir, &project_root),
                }),
                version: None,
                engines: None,
                cpu: None,
                os: None,
                libc: None,
                deprecated: None,
                has_bin: None,
                prepare: None,
                bundled_dependencies: None,
                peer_dependencies: None,
                peer_dependencies_meta: None,
            },
        );
    }

    let mut snapshots = HashMap::new();
    if let Some(input_snapshots) = lockfile.snapshots.as_ref() {
        for (key, snapshot) in input_snapshots {
            let output_key = convert_package_key(key, &ctx)?;
            snapshots.insert(output_key, convert_snapshot(snapshot, &ctx, lockfile_dir)?);
        }
    }
    // Indexed on the same components `same_path` compares, so the importer loop
    // below costs one lookup per importer rather than a scan of every project.
    let peer_bearing_projects = selected
        .all_projects
        .iter()
        .filter(|project| !project.peer_dependencies.is_empty())
        .map(|project| (comparable_path_components(&project.root_dir), project))
        .collect::<HashMap<_, _>>();
    let mut linked_workspace_projects = HashMap::new();
    for (importer_path, project_snapshot) in &lockfile.importers {
        if importer_path == project_id {
            continue;
        }
        let project_root =
            validate_lockfile_local_path(&lockfile_dir.join(importer_path), lockfile_dir)?;
        let bases = ResolveBases { file_base: lockfile_dir, link_base: &project_root };
        let package_key = create_file_url_key(&project_root, "", &selected.all_projects, None)?;
        if let Some(project) = peer_bearing_projects.get(&comparable_path_components(&project_root))
        {
            linked_workspace_projects.insert(package_key.clone(), (*project).clone());
        }
        snapshots.insert(
            package_key,
            project_snapshot_to_snapshot_entry(project_snapshot, &ctx, &bases)?,
        );
    }

    let mut deploy_lockfile = lockfile.clone();
    // The deployed manifest contains concrete dependency versions, so catalog
    // snapshots would refer to configuration that is not copied to the target.
    deploy_lockfile.catalogs = None;
    deploy_lockfile.patched_dependencies = None;
    deploy_lockfile.overrides = None;
    deploy_lockfile.package_extensions_checksum = None;
    deploy_lockfile.pnpmfile_checksum = None;
    if let Some(settings) = deploy_lockfile.settings.as_mut() {
        settings.inject_workspace_packages = false;
    }
    deploy_lockfile.importers =
        HashMap::from([(Lockfile::ROOT_IMPORTER_KEY.to_string(), target_snapshot.clone())]);
    deploy_lockfile.packages = (!packages.is_empty()).then_some(packages);
    deploy_lockfile.snapshots = (!snapshots.is_empty()).then_some(snapshots);
    prune_deploy_lockfile_graph(&mut deploy_lockfile, dependency_groups);
    bind_singleton_peers(&mut deploy_lockfile, &linked_workspace_projects)?;

    let mut manifest = selected.project.manifest.value().clone();
    set_manifest_dependencies(&mut manifest, "dependencies", target_snapshot.dependencies.as_ref());
    set_manifest_dependencies(
        &mut manifest,
        "devDependencies",
        target_snapshot.dev_dependencies.as_ref(),
    );
    set_manifest_dependencies(
        &mut manifest,
        "optionalDependencies",
        target_snapshot.optional_dependencies.as_ref(),
    );
    omit_peers_of_excluded_dependencies(&mut manifest, &declared_dependencies, &target_snapshot);

    let mut workspace_manifest = Map::new();
    let mut workspace_config =
        DeployWorkspaceConfig { patched_dependencies: None, allow_builds: HashMap::new() };
    if lockfile.patched_dependencies.is_some()
        && let Some(patched_dependencies) = config.patched_dependencies.as_ref()
    {
        deploy_lockfile.patched_dependencies.clone_from(&lockfile.patched_dependencies);
        let rewritten = patched_dependencies
            .iter()
            .map(|(name, value)| {
                let absolute = if Path::new(value).is_absolute() {
                    PathBuf::from(value)
                } else {
                    lockfile_dir.join(value)
                };
                (name.clone(), relative_path(deploy_dir, &absolute))
            })
            .collect::<indexmap::IndexMap<_, _>>();
        workspace_manifest.insert(
            "patchedDependencies".to_string(),
            serde_json::to_value(&rewritten).into_diagnostic()?,
        );
        workspace_config.patched_dependencies = Some(rewritten);
    }
    if !config.allow_builds.is_empty() {
        workspace_manifest.insert(
            "allowBuilds".to_string(),
            serde_json::to_value(&config.allow_builds).into_diagnostic()?,
        );
        workspace_config.allow_builds.clone_from(&config.allow_builds);
    }

    Ok(DeployFiles {
        manifest,
        lockfile: deploy_lockfile,
        workspace_manifest: (!workspace_manifest.is_empty())
            .then_some(Value::Object(workspace_manifest)),
        workspace_config,
    })
}

/// Keep only the dependency graph that the deploy install will materialize.
///
/// The deploy importer already carries just the included dependency groups, so
/// this walks it in full: `deploy --prod` excludes dev-only and unrelated
/// workspace snapshots from both the lockfile and the localized virtual store.
/// A linked workspace package has no package snapshot in the shared lockfile,
/// so the importer its deployed snapshot is synthesized from carries no peer
/// bindings. Bind each still-unresolved peer to the deployed graph's own
/// resolution while that resolution is unambiguous, and refuse when it is not:
/// picking between candidates is precisely the decision that injecting the
/// package would have made, and it cannot be recovered afterwards.
fn bind_singleton_peers(
    lockfile: &mut Lockfile,
    linked_workspace_projects: &HashMap<PkgNameVerPeer, ProjectInfo>,
) -> miette::Result<()> {
    if linked_workspace_projects.is_empty() {
        return Ok(());
    }
    let Some(snapshots) = lockfile.snapshots.as_ref() else { return Ok(()) };

    // Keyed by the resolved snapshot key rather than the reference that spelled
    // it, so an npm-aliased edge and a plain one that name the same package
    // count once.
    let mut candidates: HashMap<PkgName, HashSet<PkgNameVerPeer>> = HashMap::new();
    let mut record = |key: PkgNameVerPeer| {
        candidates.entry(key.name.clone()).or_default().insert(key);
    };
    if let Some(importer) = lockfile.importers.get(Lockfile::ROOT_IMPORTER_KEY) {
        for dependencies in [
            importer.dependencies.as_ref(),
            importer.dev_dependencies.as_ref(),
            importer.optional_dependencies.as_ref(),
        ]
        .into_iter()
        .flatten()
        {
            for (alias, dependency) in dependencies {
                if let Some(key) = dependency.version.resolved_key(alias) {
                    record(key);
                }
            }
        }
    }
    for snapshot in snapshots.values() {
        for dependencies in
            [snapshot.dependencies.as_ref(), snapshot.optional_dependencies.as_ref()]
                .into_iter()
                .flatten()
        {
            for (alias, dependency) in dependencies {
                if let Some(key) = dependency.resolve(alias) {
                    record(key);
                }
            }
        }
    }

    let mut bindings = Vec::new();
    for (package_key, project) in linked_workspace_projects {
        if !snapshots.contains_key(package_key) {
            continue;
        }
        for peer in &project.peer_dependencies {
            // Either map already binding the peer counts: re-binding one the
            // package declares as an optional dependency would copy it into the
            // required map and quietly promote it.
            // The graph prune clears the optional map before this runs, so a
            // peer the package depends on optionally is invisible in the
            // snapshot under `--no-optional`. Binding it there would resurrect
            // a dependency the flag excluded.
            if project.declared_dependencies.contains(peer) {
                continue;
            }
            let bound = snapshots.get(package_key).is_some_and(|snapshot| {
                [snapshot.dependencies.as_ref(), snapshot.optional_dependencies.as_ref()]
                    .into_iter()
                    .flatten()
                    .any(|dependencies| dependencies.contains_key(peer))
            });
            if bound {
                continue;
            }
            // A peer the deployed graph does not provide at all stays
            // unresolved, exactly as it is in the workspace this deploy was
            // taken from.
            let Some(resolutions) = candidates.get(peer) else { continue };
            let mut versions =
                resolutions.iter().map(|key| key.suffix.to_string()).collect::<Vec<_>>();
            if versions.len() > 1 {
                versions.sort();
                return Err(DeployError::AmbiguousPeer {
                    package: project.name.clone().unwrap_or_else(|| package_key.to_string()),
                    peer: peer.to_string(),
                    versions: versions.join(", "),
                }
                .into());
            }
            if let Some(resolution) = resolutions.iter().next() {
                bindings.push((
                    package_key.clone(),
                    peer.clone(),
                    SnapshotDepRef::Plain(resolution.suffix.clone()),
                ));
            }
        }
    }

    let Some(snapshots) = lockfile.snapshots.as_mut() else { return Ok(()) };
    for (package_key, peer, reference) in bindings {
        if let Some(snapshot) = snapshots.get_mut(&package_key) {
            snapshot.dependencies.get_or_insert_default().insert(peer, reference);
        }
    }
    Ok(())
}

fn prune_deploy_lockfile_graph(lockfile: &mut Lockfile, dependency_groups: &[DependencyGroup]) {
    let Some(snapshots) = lockfile.snapshots.as_ref() else { return };
    let Some(importer) = lockfile.importers.get(Lockfile::ROOT_IMPORTER_KEY) else { return };

    let include_optional = dependency_groups.contains(&DependencyGroup::Optional);
    let mut queue = VecDeque::new();

    {
        let mut enqueue_importer_map = |dependencies: Option<&ResolvedDependencyMap>| {
            for (alias, dependency) in dependencies.into_iter().flatten() {
                let Some(key) = dependency.version.resolved_key(alias) else { continue };
                if snapshots.contains_key(&key) {
                    queue.push_back(key);
                }
            }
        };
        enqueue_importer_map(importer.dependencies.as_ref());
        enqueue_importer_map(importer.dev_dependencies.as_ref());
        enqueue_importer_map(importer.optional_dependencies.as_ref());
    }

    let mut reachable = HashSet::new();
    while let Some(key) = queue.pop_front() {
        if !reachable.insert(key.clone()) {
            continue;
        }
        let Some(snapshot) = snapshots.get(&key) else { continue };
        for dependencies in
            snapshot.dependencies.as_ref().into_iter().chain(
                include_optional.then_some(snapshot.optional_dependencies.as_ref()).flatten(),
            )
        {
            for (alias, dependency) in dependencies {
                let Some(child) = dependency.resolve(alias) else { continue };
                if snapshots.contains_key(&child) {
                    queue.push_back(child);
                }
            }
        }
    }

    let reachable_metadata = reachable.iter().map(PackageKey::without_peer).collect::<HashSet<_>>();
    if let Some(snapshots) = lockfile.snapshots.as_mut() {
        snapshots.retain(|key, _| reachable.contains(key));
        if !include_optional {
            // A retained snapshot's optional edges point at packages this prune just dropped.
            for snapshot in snapshots.values_mut() {
                snapshot.optional_dependencies = None;
            }
        }
        if snapshots.is_empty() {
            lockfile.snapshots = None;
        }
    }
    if let Some(packages) = lockfile.packages.as_mut() {
        packages.retain(|key, _| reachable_metadata.contains(key));
        if packages.is_empty() {
            lockfile.packages = None;
        }
    }
}

/// A lockfile importer records a dependency group only when it has entries.
fn drop_empty_dependency_map(dependencies: &mut Option<ResolvedDependencyMap>) {
    if dependencies.as_ref().is_some_and(HashMap::is_empty) {
        *dependencies = None;
    }
}

fn fill_target_dependency_map<'a>(
    output: &mut Option<ResolvedDependencyMap>,
    input: impl Iterator<Item = (&'a PkgName, &'a ResolvedDependencySpec)>,
    ctx: &ConvertCtx,
    bases: &ResolveBases,
) -> miette::Result<()> {
    let output = output.get_or_insert_with(HashMap::new);
    for (name, spec) in input {
        output.insert(name.clone(), convert_resolved_dependency_spec(name, spec, ctx, bases)?);
    }
    Ok(())
}

fn set_manifest_dependencies(
    manifest: &mut Value,
    field: &str,
    dependencies: Option<&ResolvedDependencyMap>,
) {
    let deps = dependencies
        .into_iter()
        .flatten()
        .map(|(name, spec)| (name.to_string(), Value::String(spec.version.to_string())))
        .collect::<Map<_, _>>();
    if let Some(object) = manifest.as_object_mut() {
        object.insert(field.to_string(), Value::Object(deps));
    }
}

fn omit_peers_of_excluded_dependencies(
    manifest: &mut Value,
    declared_dependencies: &HashSet<String>,
    target_snapshot: &ProjectSnapshot,
) {
    let included_dependencies = dependency_names(target_snapshot);
    let excluded_dependencies =
        declared_dependencies.difference(&included_dependencies).cloned().collect::<HashSet<_>>();
    let Some(manifest) = manifest.as_object_mut() else { return };
    for field in ["peerDependencies", "peerDependenciesMeta"] {
        if let Some(Value::Object(dependencies)) = manifest.get_mut(field) {
            dependencies.retain(|name, _| !excluded_dependencies.contains(name));
        }
    }
}

fn dependency_names(snapshot: &ProjectSnapshot) -> HashSet<String> {
    snapshot
        .dependencies
        .iter()
        .flatten()
        .chain(snapshot.dev_dependencies.iter().flatten())
        .chain(snapshot.optional_dependencies.iter().flatten())
        .map(|(name, _)| name.to_string())
        .collect()
}

fn convert_package_metadata(
    metadata: &PackageMetadata,
    ctx: &ConvertCtx,
) -> miette::Result<PackageMetadata> {
    let mut metadata = metadata.clone();
    metadata.resolution = match &metadata.resolution {
        LockfileResolution::Directory(resolution) => {
            let resolved = validate_lockfile_local_path(
                &ctx.lockfile_dir.join(&resolution.directory),
                ctx.lockfile_dir,
            )?;
            LockfileResolution::Directory(DirectoryResolution {
                directory: relative_path(ctx.deploy_dir, &resolved),
            })
        }
        LockfileResolution::Tarball(resolution) if resolution.tarball.starts_with("file:") => {
            let input_path = resolution.tarball.trim_start_matches("file:");
            let resolved =
                validate_lockfile_local_path(&ctx.lockfile_dir.join(input_path), ctx.lockfile_dir)?;
            LockfileResolution::Tarball(TarballResolution {
                tarball: format!("file:{}", relative_path(ctx.deploy_dir, &resolved)),
                integrity: resolution.integrity.clone(),
                revision: None,
                git_hosted: resolution.git_hosted,
                path: resolution.path.as_ref().map(|_| relative_path(ctx.deploy_dir, &resolved)),
            })
        }
        _ => metadata.resolution.clone(),
    };
    metadata.peer_dependencies = metadata.peer_dependencies.clone();
    Ok(metadata)
}

fn convert_snapshot(
    snapshot: &SnapshotEntry,
    ctx: &ConvertCtx,
    link_base: &Path,
) -> miette::Result<SnapshotEntry> {
    let bases = ResolveBases { file_base: ctx.lockfile_dir, link_base };
    Ok(SnapshotEntry {
        dependencies: convert_snapshot_dep_map(snapshot.dependencies.as_ref(), ctx, &bases)?,
        optional_dependencies: convert_snapshot_dep_map(
            snapshot.optional_dependencies.as_ref(),
            ctx,
            &bases,
        )?,
        ..snapshot.clone()
    })
}

fn project_snapshot_to_snapshot_entry(
    snapshot: &ProjectSnapshot,
    ctx: &ConvertCtx,
    bases: &ResolveBases,
) -> miette::Result<SnapshotEntry> {
    Ok(SnapshotEntry {
        dependencies: convert_importer_dep_map_to_snapshot_deps(
            snapshot.dependencies.as_ref(),
            ctx,
            bases,
        )?,
        optional_dependencies: convert_importer_dep_map_to_snapshot_deps(
            snapshot.optional_dependencies.as_ref(),
            ctx,
            bases,
        )?,
        ..Default::default()
    })
}

fn convert_resolved_dependency_spec(
    name: &PkgName,
    spec: &ResolvedDependencySpec,
    ctx: &ConvertCtx,
    bases: &ResolveBases,
) -> miette::Result<ResolvedDependencySpec> {
    let mut spec = spec.clone();
    spec.version = convert_importer_dep_version(name, &spec.version, ctx, bases)?;
    spec.specifier = spec.version.to_string();
    Ok(spec)
}

fn convert_importer_dep_map_to_snapshot_deps(
    input: Option<&ResolvedDependencyMap>,
    ctx: &ConvertCtx,
    bases: &ResolveBases,
) -> miette::Result<Option<HashMap<PkgName, SnapshotDepRef>>> {
    let Some(input) = input else { return Ok(None) };
    let mut output = HashMap::new();
    for (name, spec) in input {
        output.insert(
            name.clone(),
            convert_importer_version_to_snapshot_ref(name, &spec.version, ctx, bases)?,
        );
    }
    Ok((!output.is_empty()).then_some(output))
}

fn convert_snapshot_dep_map(
    input: Option<&HashMap<PkgName, SnapshotDepRef>>,
    ctx: &ConvertCtx,
    bases: &ResolveBases,
) -> miette::Result<Option<HashMap<PkgName, SnapshotDepRef>>> {
    let Some(input) = input else { return Ok(None) };
    let mut output = HashMap::new();
    for (name, dep_ref) in input {
        output.insert(name.clone(), convert_snapshot_dep_ref(name, dep_ref, ctx, bases)?);
    }
    Ok((!output.is_empty()).then_some(output))
}

fn convert_importer_dep_version(
    alias: &PkgName,
    version: &ImporterDepVersion,
    ctx: &ConvertCtx,
    bases: &ResolveBases,
) -> miette::Result<ImporterDepVersion> {
    if let Some(local) = resolve_importer_dep_version(version, bases) {
        return local_to_importer_dep_version(alias, &local, ctx);
    }
    Ok(version.clone())
}

fn convert_importer_version_to_snapshot_ref(
    alias: &PkgName,
    version: &ImporterDepVersion,
    ctx: &ConvertCtx,
    bases: &ResolveBases,
) -> miette::Result<SnapshotDepRef> {
    if let Some(local) = resolve_importer_dep_version(version, bases) {
        return local_to_snapshot_dep_ref(alias, &local, ctx);
    }
    Ok(match version {
        ImporterDepVersion::Regular(version) => SnapshotDepRef::Plain(version.clone()),
        ImporterDepVersion::Alias(alias) => SnapshotDepRef::Alias(alias.clone()),
        ImporterDepVersion::Link(target) => SnapshotDepRef::Link(target.clone()),
        ImporterDepVersion::File(payload) => {
            let local = resolve_file_payload(bases.file_base, payload).with_alias(alias);
            local_to_snapshot_dep_ref(alias, &local, ctx)?
        }
    })
}

fn convert_snapshot_dep_ref(
    alias: &PkgName,
    dep_ref: &SnapshotDepRef,
    ctx: &ConvertCtx,
    bases: &ResolveBases,
) -> miette::Result<SnapshotDepRef> {
    if let Some(local) = resolve_snapshot_dep_ref(alias, dep_ref, bases) {
        return local_to_snapshot_dep_ref(alias, &local, ctx);
    }
    Ok(dep_ref.clone())
}

fn resolve_importer_dep_version(
    version: &ImporterDepVersion,
    bases: &ResolveBases,
) -> Option<LocalResolve> {
    match version {
        ImporterDepVersion::Regular(version) => resolve_pkg_ver_peer(version, bases.file_base),
        ImporterDepVersion::Alias(key) => resolve_pkg_ver_peer(&key.suffix, bases.file_base)
            .map(|local| local.with_alias(&key.name)),
        ImporterDepVersion::Link(target) => Some(resolve_link_payload(bases.link_base, target)),
        ImporterDepVersion::File(payload) => Some(resolve_file_payload(bases.file_base, payload)),
    }
}

fn resolve_snapshot_dep_ref(
    alias: &PkgName,
    dep_ref: &SnapshotDepRef,
    bases: &ResolveBases,
) -> Option<LocalResolve> {
    match dep_ref {
        SnapshotDepRef::Plain(version) => {
            resolve_pkg_ver_peer(version, bases.file_base).map(|local| local.with_alias(alias))
        }
        SnapshotDepRef::Alias(key) => resolve_pkg_ver_peer(&key.suffix, bases.file_base)
            .map(|local| local.with_alias(&key.name)),
        SnapshotDepRef::Link(target) => Some(resolve_link_payload(bases.link_base, target)),
    }
}

fn resolve_pkg_ver_peer(version: &pnpm_lockfile::PkgVerPeer, base: &Path) -> Option<LocalResolve> {
    let VersionPart::File(path) = version.version() else { return None };
    Some(LocalResolve {
        resolved_path: lexical_normalize(&base.join(path)),
        suffix: version.peer().to_string(),
    })
}

fn resolve_file_payload(base: &Path, payload: &str) -> LocalResolve {
    let (path, suffix) = split_local_payload(payload);
    LocalResolve { resolved_path: lexical_normalize(&base.join(path)), suffix: suffix.to_string() }
}

fn resolve_link_payload(base: &Path, payload: &str) -> LocalResolve {
    let (path, suffix) = split_local_payload(payload);
    LocalResolve { resolved_path: lexical_normalize(&base.join(path)), suffix: suffix.to_string() }
}

fn split_local_payload(payload: &str) -> (&str, &str) {
    let suffix = pnpm_deps_path::index_of_dep_path_suffix(payload);
    match suffix.patch_hash_index.or(suffix.peers_index) {
        Some(index) => (&payload[..index], &payload[index..]),
        None => (payload, ""),
    }
}

impl LocalResolve {
    fn with_alias(self, _alias: &PkgName) -> Self {
        self
    }
}

fn local_to_importer_dep_version(
    alias: &PkgName,
    local: &LocalResolve,
    ctx: &ConvertCtx,
) -> miette::Result<ImporterDepVersion> {
    let resolved_path = validate_lockfile_local_path(&local.resolved_path, ctx.lockfile_dir)?;
    if same_path(&resolved_path, ctx.deployed_project_root) {
        return Ok(ImporterDepVersion::Link(".".to_string()));
    }
    let key = create_file_url_key(&resolved_path, &local.suffix, ctx.all_projects, Some(alias))?;
    Ok(ImporterDepVersion::Alias(key))
}

fn local_to_snapshot_dep_ref(
    alias: &PkgName,
    local: &LocalResolve,
    ctx: &ConvertCtx,
) -> miette::Result<SnapshotDepRef> {
    let resolved_path = validate_lockfile_local_path(&local.resolved_path, ctx.lockfile_dir)?;
    if same_path(&resolved_path, ctx.deployed_project_root) {
        return Ok(SnapshotDepRef::Link(".".to_string()));
    }
    Ok(SnapshotDepRef::Alias(create_file_url_key(
        &resolved_path,
        &local.suffix,
        ctx.all_projects,
        Some(alias),
    )?))
}

fn convert_package_key(key: &PackageKey, ctx: &ConvertCtx) -> miette::Result<PackageKey> {
    let VersionPart::File(path) = key.suffix.version() else { return Ok(key.clone()) };
    let resolved = validate_lockfile_local_path(&ctx.lockfile_dir.join(path), ctx.lockfile_dir)?;
    create_file_url_key(&resolved, key.suffix.peer(), ctx.all_projects, Some(&key.name))
}

fn validate_lockfile_local_path(path: &Path, lockfile_dir: &Path) -> miette::Result<PathBuf> {
    let normalized = lexical_normalize(path);
    let workspace_dir = lexical_normalize(lockfile_dir);
    if same_path(&normalized, &workspace_dir) || is_child_path(&normalized, &workspace_dir) {
        return Ok(normalized);
    }
    Err(DeployError::UnsafeLockfilePath { path: normalized, workspace_dir }.into())
}

fn create_file_url_key(
    resolved_path: &Path,
    suffix: &str,
    all_projects: &[ProjectInfo],
    package_name: Option<&PkgName>,
) -> miette::Result<PkgNameVerPeer> {
    let normalized = lexical_normalize(resolved_path);
    let normalized_display = normalized.display();
    let dep_file_url = url::Url::from_file_path(&normalized)
        .map_err(|()| miette::miette!("could not convert {} to a file URL", normalized_display))?
        .to_string();
    let name = all_projects
        .iter()
        .find(|project| same_path(&project.root_dir, &normalized))
        .and_then(|project| project.name.as_deref())
        .map(str::to_string)
        .or_else(|| package_name.map(PkgName::to_string))
        .or_else(|| normalized.file_name().map(|name| name.to_string_lossy().into_owned()))
        .unwrap_or_else(|| normalized.display().to_string());
    format!("{name}@{dep_file_url}{suffix}")
        .parse()
        .into_diagnostic()
        .wrap_err("create deploy file URL dependency path")
}

fn same_path(left: &Path, right: &Path) -> bool {
    let left = lexical_normalize(left);
    let right = lexical_normalize(right);
    path_components_match(&left, &right)
}

fn has_path_prefix(child: &Path, parent: &Path) -> bool {
    let child = lexical_normalize(child);
    let parent = lexical_normalize(parent);
    let child_components = comparable_path_components(&child);
    let parent_components = comparable_path_components(&parent);
    child_components.len() >= parent_components.len()
        && child_components
            .iter()
            .zip(parent_components.iter())
            .all(|(child, parent)| child == parent)
}

fn path_components_match(left: &Path, right: &Path) -> bool {
    comparable_path_components(left) == comparable_path_components(right)
}

fn comparable_path_components(path: &Path) -> Vec<String> {
    path.components()
        .map(|component| comparison_component(component.as_os_str().to_string_lossy().as_ref()))
        .collect()
}

#[cfg(windows)]
fn comparison_component(component: &str) -> String {
    component.to_lowercase()
}

#[cfg(not(windows))]
fn comparison_component(component: &str) -> String {
    component.to_string()
}

fn relative_components_from_child(parent: &Path, child: &Path) -> miette::Result<Vec<PathBuf>> {
    let parent = lexical_normalize(parent);
    let child = lexical_normalize(child);
    if !has_path_prefix(&child, &parent) {
        child.strip_prefix(&parent).into_diagnostic()?;
    }
    Ok(child
        .components()
        .skip(parent.components().count())
        .map(|component| PathBuf::from(component.as_os_str()))
        .collect())
}

fn relative_path(from: &Path, to: &Path) -> String {
    let relative = pathdiff::diff_paths(to, from).unwrap_or_else(|| to.to_path_buf());
    relative.to_string_lossy().replace('\\', "/")
}

fn write_deploy_files(deploy_dir: &Path, deploy_files: &DeployFiles) -> miette::Result<()> {
    let mut manifest = serde_json::to_string_pretty(&deploy_files.manifest).into_diagnostic()?;
    manifest.push('\n');
    let lockfile = deploy_files
        .lockfile
        .to_yaml_string()
        .map_err(miette::Report::new)
        .wrap_err("serialize deployed lockfile")?;
    write_atomic(&deploy_dir.join(Lockfile::FILE_NAME), lockfile.as_bytes())
        .into_diagnostic()
        .wrap_err("write deployed lockfile")?;
    if let Some(workspace_manifest) = &deploy_files.workspace_manifest {
        write_atomic(
            &deploy_dir.join(WORKSPACE_MANIFEST_FILENAME),
            workspace_manifest_yaml(workspace_manifest).as_bytes(),
        )
        .into_diagnostic()
        .wrap_err("write deployed workspace manifest")?;
    }
    write_atomic(&deploy_dir.join("package.json"), manifest.as_bytes())
        .into_diagnostic()
        .wrap_err("write deployed package.json")?;
    Ok(())
}

fn write_atomic(path: &Path, contents: &[u8]) -> io::Result<()> {
    let dir = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
    tmp.write_all(contents)?;
    tmp.as_file().sync_all()?;
    if let Ok(metadata) = fs::metadata(path) {
        tmp.as_file().set_permissions(metadata.permissions())?;
    }
    tmp.persist(path).map_err(|error| error.error)?;
    Ok(())
}

fn workspace_manifest_yaml(workspace_manifest: &Value) -> String {
    let mut out = String::new();
    let Some(object) = workspace_manifest.as_object() else { return out };
    for field in ["patchedDependencies", "allowBuilds"] {
        let Some(values) = object.get(field).and_then(Value::as_object) else { continue };
        out.push_str(field);
        out.push_str(":\n");
        for (key, value) in values {
            out.push_str("  ");
            out.push_str(&serde_json::to_string(key).unwrap_or_else(|_| format!("{key:?}")));
            out.push_str(": ");
            out.push_str(&serde_json::to_string(value).unwrap_or_else(|_| value.to_string()));
            out.push('\n');
        }
    }
    out
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
