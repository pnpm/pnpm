use crate::{
    State,
    cli_args::install::{InstallArgs, NodeLinkerArg},
};
use clap::Args;
use derive_more::{Display, Error};
use indexmap::IndexMap;
use miette::{Context, Diagnostic, IntoDiagnostic};
use pacquet_config::{Config, LinkWorkspacePackages, NodeLinker, PackageImportMethod};
use pacquet_directory_fetcher::DirectoryFetcher;
use pacquet_fs::lexical_normalize;
use pacquet_lockfile::{
    DirectoryResolution, ImporterDepVersion, Lockfile, LockfileResolution, MaybeLazyLockfile,
    PackageKey, PackageMetadata, PkgName, PkgNameVerPeer, ProjectSnapshot, ResolvedDependencyMap,
    ResolvedDependencySpec, SnapshotDepRef, SnapshotEntry, TarballResolution, VersionPart,
};
use pacquet_package_manager::{
    ImportIndexedDirOpts, Install, UpdateSeedPolicy, import_indexed_dir,
};
use pacquet_package_manifest::{DependencyGroup, PackageManifest};
use pacquet_reporter::{LogEvent, LogLevel, PnpmLog, Reporter};
use pacquet_workspace::{
    FindWorkspaceProjectsOpts, Project, WORKSPACE_MANIFEST_FILENAME, find_workspace_projects,
    importer_id_from_root_dir, read_workspace_manifest, workspace_package_patterns,
};
use pacquet_workspace_projects_filter::{FilterProjectsOptions, WorkspaceFilter, filter_projects};
use pacquet_workspace_projects_graph::{BaseProject, GraphProject};
use serde_json::{Map, Value, json};
use std::{
    collections::HashMap,
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{Arc, atomic::AtomicU8},
};

#[derive(Debug, Args)]
pub struct DeployArgs {
    #[clap(flatten)]
    pub install_args: InstallArgs,

    /// Use the legacy install-based deploy implementation.
    #[clap(long)]
    pub legacy: bool,

    /// Delete the deploy path when it already exists and is not empty.
    #[clap(long)]
    pub force: bool,

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
        r#"By default, starting from pnpm v10, we only deploy from workspaces that have "inject-workspace-packages=true" set"#
    )]
    #[diagnostic(
        code(ERR_PNPM_DEPLOY_NONINJECTED_WORKSPACE),
        help(
            r#"If you want to deploy without using injected dependencies, run "pnpm deploy" with the "--legacy" flag or set "force-legacy-deploy" to true"#
        )
    )]
    NonInjectedWorkspace,

    #[display("The selected project is missing from pnpm-lock.yaml: {project_id}")]
    #[diagnostic(code(ERR_PNPM_CANNOT_DEPLOY))]
    MissingImporter { project_id: String },
}

#[derive(Clone, Copy)]
struct GraphPkg<'a> {
    project: &'a Project,
}

impl BaseProject for GraphPkg<'_> {
    fn root_dir(&self) -> &Path {
        &self.project.root_dir
    }

    fn manifest_name(&self) -> Option<&str> {
        self.project.manifest.value().get("name").and_then(Value::as_str)
    }
}

impl GraphProject for GraphPkg<'_> {
    fn manifest_version(&self) -> Option<&str> {
        self.project.manifest.value().get("version").and_then(Value::as_str)
    }

    fn merged_dependencies(&self, ignore_dev_deps: bool) -> Vec<(String, String)> {
        let mut merged: IndexMap<String, String> = IndexMap::new();
        let mut absorb = |group: DependencyGroup| {
            for (name, spec) in self.project.manifest.dependencies([group]) {
                merged.insert(name.to_string(), spec.to_string());
            }
        };
        absorb(DependencyGroup::Peer);
        if !ignore_dev_deps {
            absorb(DependencyGroup::Dev);
        }
        absorb(DependencyGroup::Optional);
        absorb(DependencyGroup::Prod);
        merged.into_iter().collect()
    }
}

#[derive(Clone)]
struct ProjectInfo {
    root_dir: PathBuf,
    name: Option<String>,
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
        let selected = select_project(config, workspace_dir)?;
        if self.target_dirs.len() != 1 {
            return Err(DeployError::InvalidDeployTarget.into());
        }

        let force_legacy = self.legacy || config.force_legacy_deploy;
        if config.shared_workspace_lockfile && !force_legacy && !config.inject_workspace_packages {
            return Err(DeployError::NonInjectedWorkspace.into());
        }

        let deploy_dir = resolve_target_dir(dir, &self.target_dirs[0]);
        validate_deploy_target(
            &deploy_dir,
            workspace_dir,
            &selected.project.root_dir,
            dir,
            self.force,
        )?;
        prepare_deploy_dir::<ReporterT>(&deploy_dir, self.force)?;
        copy_project::<ReporterT>(
            &selected.project.root_dir,
            &deploy_dir,
            !config.deploy_all_files,
        )?;

        if config.shared_workspace_lockfile && !force_legacy {
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
        } else if config.shared_workspace_lockfile && force_legacy {
            warn::<ReporterT>(
                &deploy_dir,
                "Shared workspace lockfile detected but configuration forces legacy deploy implementation.",
            );
        }

        apply_deploy_hook(&deploy_dir.join("package.json"))?;
        self.run_install_in_deploy_dir::<ReporterT>(
            config,
            &deploy_dir,
            DeployInstallMode::Legacy,
            false,
        )
        .await
    }

    async fn deploy_from_shared_lockfile<ReporterT: Reporter + 'static>(
        &self,
        config: &'static Config,
        workspace_dir: &Path,
        selected: &SelectedProject,
        deploy_dir: &Path,
    ) -> miette::Result<SharedDeployOutcome> {
        if !config.inject_workspace_packages {
            return Err(DeployError::NonInjectedWorkspace.into());
        }
        let Some(lockfile) = Lockfile::load_wanted_from_dir(workspace_dir)
            .map_err(miette::Report::new)
            .wrap_err("read shared lockfile")?
        else {
            return Ok(SharedDeployOutcome::Fallback(
                "Shared lockfile not found. Falling back to installing without a lockfile."
                    .to_string(),
            ));
        };

        let project_id = importer_id_from_root_dir(workspace_dir, &selected.project.root_dir);
        let deploy_files = create_deploy_files(
            &lockfile,
            selected,
            &project_id,
            workspace_dir,
            deploy_dir,
            config,
        )?;
        write_deploy_files(deploy_dir, &deploy_files)?;
        self.run_install_in_deploy_dir::<ReporterT>(
            config,
            deploy_dir,
            DeployInstallMode::Shared { workspace_config: deploy_files.workspace_config },
            true,
        )
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
        let mut deploy_config = base_config.clone();
        deploy_config.modules_dir = deploy_dir.join("node_modules");
        deploy_config.virtual_store_dir = deploy_dir.join("node_modules/.pnpm");
        deploy_config.global_virtual_store_dir = deploy_config.virtual_store_dir.clone();
        deploy_config.enable_global_virtual_store = false;
        deploy_config.pnpr_server = None;
        deploy_config.optimistic_repeat_install = false;
        deploy_config.dedupe_peer_dependents = false;
        deploy_config.dedupe_injected_deps = false;
        deploy_config.node_linker = node_linker;
        deploy_config.lockfile = !matches!(node_linker, NodeLinker::Hoisted if !frozen_lockfile);
        deploy_config.prefer_frozen_lockfile = frozen_lockfile;

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
        let state = State::init(deploy_dir.join("package.json"), deploy_config, frozen_lockfile)
            .wrap_err("initialize the deploy install state")?;
        let State { tarball_mem_cache, http_client, config, manifest, lockfile, resolved_packages } =
            &state;

        let supported_architectures = self
            .install_args
            .supported_architectures
            .apply_to(config.supported_architectures.clone());
        let skip_runtimes = config.skip_runtimes || self.install_args.no_runtime;
        let trust_lockfile = config.trust_lockfile || self.install_args.trust_lockfile;
        let lockfile_path = config.lockfile.then(|| deploy_dir.join(Lockfile::FILE_NAME));
        let prefer_frozen_lockfile = frozen_lockfile.then_some(true).or(Some(false));
        let dependency_groups =
            self.install_args.dependency_options.dependency_groups().collect::<Vec<_>>();

        Install {
            tarball_mem_cache: Arc::clone(tarball_mem_cache),
            http_client,
            http_client_arc: Arc::clone(http_client),
            config,
            manifest,
            lockfile: MaybeLazyLockfile::Lazy(lockfile),
            lockfile_path: lockfile_path.as_deref(),
            dependency_groups,
            frozen_lockfile,
            prefer_frozen_lockfile,
            ignore_manifest_check: false,
            skip_runtimes,
            trust_lockfile,
            update_checksums: false,
            is_full_install: true,
            resolved_packages,
            supported_architectures,
            node_linker,
            lockfile_only: false,
            dry_run: false,
            update_seed_policy: UpdateSeedPolicy::KeepAll,
            auth_override: None,
            resolution_observer: None,
            catalogs_override: None,
            disable_optimistic_repeat_install: true,
        }
        .run::<ReporterT>()
        .await
        .wrap_err("installing deployed dependencies")
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

fn select_project(config: &Config, workspace_dir: &Path) -> miette::Result<SelectedProject> {
    let manifest = read_workspace_manifest(workspace_dir)
        .map_err(miette::Report::new)
        .wrap_err("read workspace manifest")?
        .unwrap_or_default();
    let projects = find_workspace_projects(
        workspace_dir,
        &FindWorkspaceProjectsOpts { patterns: Some(workspace_package_patterns(&manifest)) },
    )
    .map_err(miette::Report::new)
    .wrap_err("find workspace projects")?;

    let all_projects = projects
        .iter()
        .map(|project| ProjectInfo {
            root_dir: lexical_normalize(&project.root_dir),
            name: project.manifest.value().get("name").and_then(Value::as_str).map(str::to_string),
        })
        .collect::<Vec<_>>();

    let graph_projects = projects.iter().map(|project| GraphPkg { project }).collect::<Vec<_>>();
    let filters =
        config
            .filter
            .iter()
            .map(|filter| WorkspaceFilter { filter: filter.clone(), follow_prod_deps_only: false })
            .chain(config.filter_prod.iter().map(|filter| WorkspaceFilter {
                filter: filter.clone(),
                follow_prod_deps_only: true,
            }))
            .collect::<Vec<_>>();
    let link_workspace_packages =
        Some(config.link_workspace_packages != LinkWorkspacePackages::Off);
    let selected = filter_projects(
        graph_projects,
        &filters,
        &FilterProjectsOptions {
            prefix: workspace_dir.to_path_buf(),
            link_workspace_packages,
            use_glob_dir_filtering: false,
        },
    )
    .map_err(miette::Report::new)
    .wrap_err("filter workspace projects")?;

    match selected.selected_projects.as_slice() {
        [] => Err(DeployError::NothingToDeploy.into()),
        [_one] if selected.selected_projects.len() == 1 => {
            let selected_root = lexical_normalize(&selected.selected_projects[0]);
            let project = projects
                .into_iter()
                .find(|project| lexical_normalize(&project.root_dir) == selected_root)
                .ok_or(DeployError::NothingToDeploy)?;
            Ok(SelectedProject { project, all_projects })
        }
        _ => Err(DeployError::CannotDeployMany.into()),
    }
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

    Ok(())
}

fn unsafe_deploy_target<Output>(deploy_dir: &Path, reason: &'static str) -> miette::Result<Output> {
    Err(DeployError::UnsafeDeployTarget { deploy_dir: deploy_dir.to_path_buf(), reason }.into())
}

fn is_ancestor_path(parent: &Path, child: &Path) -> bool {
    child.starts_with(parent) && !same_path(parent, child)
}

fn is_child_path(child: &Path, parent: &Path) -> bool {
    child.starts_with(parent) && !same_path(child, parent)
}

fn prepare_deploy_dir<ReporterT: Reporter>(deploy_dir: &Path, force: bool) -> miette::Result<()> {
    if !is_empty_dir_or_absent(deploy_dir)? {
        if !force {
            return Err(
                DeployError::DeployDirNotEmpty { deploy_dir: deploy_dir.to_path_buf() }.into()
            );
        }
        warn::<ReporterT>(
            deploy_dir,
            format!("using --force, deleting deploy path {}", deploy_dir.display()),
        );
    }
    remove_path_if_exists(deploy_dir)?;
    fs::create_dir_all(deploy_dir)
        .into_diagnostic()
        .wrap_err_with(|| format!("create deploy directory {}", deploy_dir.display()))
}

fn is_empty_dir_or_absent(path: &Path) -> miette::Result<bool> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(true),
        Err(error) => return Err(error).into_diagnostic(),
    };
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Ok(false);
    }
    let mut entries = fs::read_dir(path).into_diagnostic()?;
    Ok(entries.next().is_none())
}

fn remove_path_if_exists(path: &Path) -> miette::Result<()> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error).into_diagnostic(),
    };
    if metadata.is_dir() && !metadata.file_type().is_symlink() {
        fs::remove_dir_all(path).into_diagnostic()
    } else {
        fs::remove_file(path).into_diagnostic()
    }
    .wrap_err_with(|| format!("remove deploy path {}", path.display()))
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
        ImportIndexedDirOpts { force: true, keep_modules_dir: false },
    )
    .map_err(miette::Report::new)
    .wrap_err("copy project files")
}

fn apply_deploy_hook(manifest_path: &Path) -> miette::Result<()> {
    let mut manifest = PackageManifest::from_path(manifest_path.to_path_buf())
        .wrap_err("read deployed manifest")?;
    apply_deploy_hook_to_value(manifest.value_mut());
    manifest.save().wrap_err("write deployed manifest")
}

fn apply_deploy_hook_to_value(manifest: &mut Value) {
    let names = ["dependencies", "devDependencies", "optionalDependencies"]
        .into_iter()
        .filter_map(|field| manifest.get(field)?.as_object())
        .flat_map(|deps| deps.iter())
        .filter_map(|(name, spec)| {
            spec.as_str().is_some_and(|spec| spec.starts_with("workspace:")).then_some(name.clone())
        })
        .collect::<Vec<_>>();
    if names.is_empty() {
        return;
    }
    let Some(object) = manifest.as_object_mut() else { return };
    let dependencies_meta =
        object.entry("dependenciesMeta").or_insert_with(|| Value::Object(Map::new()));
    let Some(meta_object) = dependencies_meta.as_object_mut() else { return };
    for name in names {
        meta_object.insert(name, json!({ "injected": true }));
    }
}

fn create_deploy_files(
    lockfile: &Lockfile,
    selected: &SelectedProject,
    project_id: &str,
    lockfile_dir: &Path,
    deploy_dir: &Path,
    config: &Config,
) -> miette::Result<DeployFiles> {
    let input_snapshot = lockfile
        .importers
        .get(project_id)
        .ok_or_else(|| DeployError::MissingImporter { project_id: project_id.to_string() })?;
    let deployed_project_root = lexical_normalize(&lockfile_dir.join(project_id));
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

    let selected_root = lexical_normalize(&selected.project.root_dir);
    let selected_bases = ResolveBases { file_base: lockfile_dir, link_base: &selected_root };
    fill_target_dependency_map(
        &mut target_snapshot.dependencies,
        input_snapshot.dependencies.as_ref(),
        &ctx,
        &selected_bases,
    )?;
    fill_target_dependency_map(
        &mut target_snapshot.dev_dependencies,
        input_snapshot.dev_dependencies.as_ref(),
        &ctx,
        &selected_bases,
    )?;
    fill_target_dependency_map(
        &mut target_snapshot.optional_dependencies,
        input_snapshot.optional_dependencies.as_ref(),
        &ctx,
        &selected_bases,
    )?;

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
        let project_root = lexical_normalize(&lockfile_dir.join(importer_path));
        let package_key = create_file_url_key(&project_root, "", &selected.all_projects)?;
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
    for (importer_path, project_snapshot) in &lockfile.importers {
        if importer_path == project_id {
            continue;
        }
        let project_root = lexical_normalize(&lockfile_dir.join(importer_path));
        let bases = ResolveBases { file_base: lockfile_dir, link_base: &project_root };
        let package_key = create_file_url_key(&project_root, "", &selected.all_projects)?;
        snapshots.insert(
            package_key,
            project_snapshot_to_snapshot_entry(project_snapshot, &ctx, &bases)?,
        );
    }

    let mut deploy_lockfile = lockfile.clone();
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

fn fill_target_dependency_map(
    output: &mut Option<ResolvedDependencyMap>,
    input: Option<&ResolvedDependencyMap>,
    ctx: &ConvertCtx,
    bases: &ResolveBases,
) -> miette::Result<()> {
    let output = output.get_or_insert_with(HashMap::new);
    if let Some(input) = input {
        for (name, spec) in input {
            output.insert(name.clone(), convert_resolved_dependency_spec(name, spec, ctx, bases)?);
        }
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

fn convert_package_metadata(
    metadata: &PackageMetadata,
    ctx: &ConvertCtx,
) -> miette::Result<PackageMetadata> {
    let mut metadata = metadata.clone();
    metadata.resolution = match &metadata.resolution {
        LockfileResolution::Directory(resolution) => {
            let resolved = lexical_normalize(&ctx.lockfile_dir.join(&resolution.directory));
            LockfileResolution::Directory(DirectoryResolution {
                directory: relative_path(ctx.deploy_dir, &resolved),
            })
        }
        LockfileResolution::Tarball(resolution) if resolution.tarball.starts_with("file:") => {
            let input_path = resolution.tarball.trim_start_matches("file:");
            let resolved = lexical_normalize(&ctx.lockfile_dir.join(input_path));
            LockfileResolution::Tarball(TarballResolution {
                tarball: format!("file:{}", relative_path(ctx.deploy_dir, &resolved)),
                integrity: resolution.integrity.clone(),
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
        return local_to_snapshot_dep_ref(&local, ctx);
    }
    Ok(match version {
        ImporterDepVersion::Regular(version) => SnapshotDepRef::Plain(version.clone()),
        ImporterDepVersion::Alias(alias) => SnapshotDepRef::Alias(alias.clone()),
        ImporterDepVersion::Link(target) => SnapshotDepRef::Link(target.clone()),
        ImporterDepVersion::File(payload) => {
            let local = resolve_file_payload(bases.file_base, payload).with_alias(alias);
            local_to_snapshot_dep_ref(&local, ctx)?
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
        return local_to_snapshot_dep_ref(&local, ctx);
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

fn resolve_pkg_ver_peer(
    version: &pacquet_lockfile::PkgVerPeer,
    base: &Path,
) -> Option<LocalResolve> {
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
    let suffix = pacquet_deps_path::index_of_dep_path_suffix(payload);
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
    _alias: &PkgName,
    local: &LocalResolve,
    ctx: &ConvertCtx,
) -> miette::Result<ImporterDepVersion> {
    if same_path(&local.resolved_path, ctx.deployed_project_root) {
        return Ok(ImporterDepVersion::Link(".".to_string()));
    }
    let key = create_file_url_key(&local.resolved_path, &local.suffix, ctx.all_projects)?;
    Ok(ImporterDepVersion::Alias(key))
}

fn local_to_snapshot_dep_ref(
    local: &LocalResolve,
    ctx: &ConvertCtx,
) -> miette::Result<SnapshotDepRef> {
    if same_path(&local.resolved_path, ctx.deployed_project_root) {
        return Ok(SnapshotDepRef::Link(".".to_string()));
    }
    Ok(SnapshotDepRef::Alias(create_file_url_key(
        &local.resolved_path,
        &local.suffix,
        ctx.all_projects,
    )?))
}

fn convert_package_key(key: &PackageKey, ctx: &ConvertCtx) -> miette::Result<PackageKey> {
    let VersionPart::File(path) = key.suffix.version() else { return Ok(key.clone()) };
    let resolved = lexical_normalize(&ctx.lockfile_dir.join(path));
    create_file_url_key(&resolved, key.suffix.peer(), ctx.all_projects)
}

fn create_file_url_key(
    resolved_path: &Path,
    suffix: &str,
    all_projects: &[ProjectInfo],
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
        .or_else(|| normalized.file_name().map(|name| name.to_string_lossy().into_owned()))
        .unwrap_or_else(|| normalized.display().to_string());
    format!("{name}@{dep_file_url}{suffix}")
        .parse()
        .into_diagnostic()
        .wrap_err("create deploy file URL dependency path")
}

fn same_path(left: &Path, right: &Path) -> bool {
    lexical_normalize(left) == lexical_normalize(right)
}

fn relative_path(from: &Path, to: &Path) -> String {
    let relative = pathdiff::diff_paths(to, from).unwrap_or_else(|| to.to_path_buf());
    relative.to_string_lossy().replace('\\', "/")
}

fn write_deploy_files(deploy_dir: &Path, deploy_files: &DeployFiles) -> miette::Result<()> {
    let mut manifest = serde_json::to_string_pretty(&deploy_files.manifest).into_diagnostic()?;
    manifest.push('\n');
    deploy_files
        .lockfile
        .save_to_path(&deploy_dir.join(Lockfile::FILE_NAME))
        .map_err(miette::Report::new)
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
