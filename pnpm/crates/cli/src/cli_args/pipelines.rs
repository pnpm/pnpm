use super::{
    add::AddArgs,
    dedupe::{self, DedupeArgs},
    deploy::DeployArgs,
    install::{InstallArgs, resolve_bool_override},
    package_manager::{PackageManagerToSync, package_manager_to_sync},
    prune::PruneArgs,
    recursive::{
        AutoExcludeRoot, discover_workspace_projects, select_recursive_projects,
        sort_filtered_projects,
    },
    remove::RemoveArgs,
    update::UpdateArgs,
    update_changeset::UpdateChangesetContext,
};
use crate::{State, config_deps};
use miette::Context;
use pacquet_config::Config;
use pacquet_reporter::Reporter;
use std::{
    collections::{BTreeMap, HashSet},
    path::{Path, PathBuf},
    sync::Arc,
};

pub(crate) struct InstallFamilySelection {
    pub(crate) workspace_root: PathBuf,
    pub(crate) projects: Vec<pacquet_workspace::Project>,
    pub(crate) ordered_groups: Vec<Vec<PathBuf>>,
    pub(crate) ordered_dirs: Vec<PathBuf>,
    pub(crate) selected_dirs: Arc<HashSet<PathBuf>>,
    pub(crate) active_manifest_is_standin: bool,
}

/// How a recursive / filtered install-family command should be dispatched,
/// resolved from the config and the workspace selection.
pub(crate) enum InstallFamilyPlan {
    /// Not recursive (`!cfg.recursive`): run against the active project only.
    /// The pipelines keep their own non-recursive handling (the dedicated
    /// per-project anchor for `add` / `update` / `remove`, and the
    /// dedicated-lockfile workspace install for `install`).
    Single,
    /// Recursive / filtered over a shared workspace lockfile: one mutation
    /// pass writes every selected importer into the shared `pnpm-lock.yaml`.
    Shared(Box<InstallFamilySelection>),
    /// Recursive / filtered with one lockfile per project
    /// (`sharedWorkspaceLockfile: false`): the selected project directories,
    /// each installed independently against its own `pnpm-lock.yaml`,
    /// `node_modules`, and virtual store. Mirrors pnpm's per-project loop in
    /// its recursive dispatch. The order is not topological — each project
    /// resolves in isolation — so the dirs are sorted for a deterministic run
    /// order, matching pnpm's alphabetical `Object.keys(...).sort()`.
    PerProject(Vec<PathBuf>),
}

fn select_install_family_plan(
    cfg: &Config,
    prefix: &Path,
    manifest_path: &Path,
    recursive_sort: bool,
    auto_exclude_root: bool,
) -> miette::Result<InstallFamilyPlan> {
    if !cfg.recursive {
        return Ok(InstallFamilyPlan::Single);
    }

    let workspace_root = cfg.workspace_dir.as_deref().unwrap_or(prefix).to_path_buf();
    let (mut projects, workspace_patterns) = discover_workspace_projects(&workspace_root)?;
    if let Some(runtime_on_fail) = cfg.runtime_on_fail {
        for project in &mut projects {
            pacquet_package_manifest::apply_runtime_on_fail_override(
                project.manifest.value_mut(),
                runtime_on_fail.as_str(),
            );
        }
    }
    let (ordered_groups, ordered_dirs, selected_dirs) = {
        let selection = select_recursive_projects(
            &projects,
            cfg,
            prefix,
            if auto_exclude_root {
                AutoExcludeRoot::Enabled { workspace_patterns: workspace_patterns.as_deref() }
            } else {
                AutoExcludeRoot::Disabled
            },
        )?;
        if !cfg.shared_workspace_lockfile {
            let mut project_dirs: Vec<PathBuf> = selection.selected.keys().cloned().collect();
            project_dirs.sort();
            return Ok(InstallFamilyPlan::PerProject(project_dirs));
        }
        let ordered_groups = if recursive_sort {
            sort_filtered_projects(
                &selection.selected,
                selection.full_graph(),
                selection.prod_all.as_ref(),
                &selection.prod_only_selected,
            )
        } else {
            vec![selection.selected.keys().cloned().collect()]
        };
        let ordered_dirs = ordered_groups.iter().flatten().cloned().collect();
        let selected_dirs = Arc::new(selection.selected.keys().cloned().collect());
        (ordered_groups, ordered_dirs, selected_dirs)
    };

    let active_dir = manifest_path.parent().expect("manifest path always has a parent dir");
    let normalized_active_dir = pacquet_fs::lexical_normalize(active_dir);
    let active_manifest_is_standin = !active_dir.join("package.json").is_file()
        && pacquet_workspace::try_read_project_manifest(active_dir)
            .map_err(miette::Report::new)?
            .is_none()
        && !projects.iter().any(|project| {
            pacquet_fs::lexical_normalize(&project.root_dir) == normalized_active_dir
        });

    Ok(InstallFamilyPlan::Shared(Box::new(InstallFamilySelection {
        workspace_root,
        projects,
        ordered_groups,
        ordered_dirs,
        selected_dirs,
        active_manifest_is_standin,
    })))
}

/// Build the project-anchored `State` for one project of a
/// `sharedWorkspaceLockfile: false` workspace: clone `cfg`, re-anchor its
/// output paths under `project_dir` via [`anchor_dedicated_project_config`],
/// and initialize the state. The clone is leaked because [`State::init`] needs
/// a `&'static Config`; see [`run_dedicated_lockfile_workspace_install`] for
/// why the bounded leak is acceptable.
fn init_dedicated_project_state(
    cfg: &Config,
    project_dir: &Path,
    require_lockfile: bool,
) -> miette::Result<State> {
    let mut project_config = cfg.clone();
    anchor_dedicated_project_config(&mut project_config, project_dir);
    let project_config = Config::leak(project_config);
    State::init(project_dir.join("package.json"), project_config, require_lockfile)
        .wrap_err_with(|| format!("initialize the state for {}", project_dir.display()))
}

/// The reporter-generic body of `pacquet install`: it threads one `Reporter`
/// type through config-dependency sync, the `updateConfig` hooks, and the
/// install itself. Lifting it out of the dispatch keeps the three
/// `ReporterType` arms to a single line each.
pub(crate) struct InstallPipeline {
    pub(crate) args: InstallArgs,
    pub(crate) cfg: &'static mut Config,
    pub(crate) config_root: PathBuf,
    pub(crate) package_manager_to_sync: Option<PackageManagerToSync>,
    pub(crate) prefix: PathBuf,
    pub(crate) manifest_path: PathBuf,
    pub(crate) recursive_sort: bool,
    pub(crate) require_lockfile: bool,
    pub(crate) frozen_lockfile: bool,
}

impl InstallPipeline {
    pub(crate) async fn run<Reporter: self::Reporter + 'static>(self) -> miette::Result<()> {
        let InstallPipeline {
            args,
            cfg,
            config_root,
            package_manager_to_sync,
            prefix,
            manifest_path,
            recursive_sort,
            require_lockfile,
            frozen_lockfile,
        } = self;
        if let Some(pm) = package_manager_to_sync.as_ref() {
            config_deps::sync_package_manager_dependencies(
                cfg,
                &config_root,
                &pm.specifier,
                &pm.version,
                frozen_lockfile,
            )
            .await?;
        }
        config_deps::install_config_deps::<Reporter>(cfg, &config_root, frozen_lockfile).await?;
        config_deps::run_update_config_hooks::<Reporter>(cfg, &config_root).await?;
        let plan = select_install_family_plan(cfg, &prefix, &manifest_path, recursive_sort, false)?;
        match plan {
            InstallFamilyPlan::PerProject(project_dirs) => {
                let cfg: &Config = cfg;
                for project_dir in project_dirs {
                    let state = init_dedicated_project_state(cfg, &project_dir, require_lockfile)?;
                    Box::pin(args.clone().run::<Reporter>(state)).await?;
                }
                Ok(())
            }
            InstallFamilyPlan::Shared(selection) => {
                if selection.selected_dirs.is_empty() {
                    return Ok(());
                }
                let cfg: &'static Config = cfg;
                let state = State::init(manifest_path, cfg, require_lockfile)
                    .wrap_err("initialize the state")?;
                Box::pin(args.run_selected::<Reporter>(state, *selection)).await
            }
            InstallFamilyPlan::Single => {
                if !cfg.shared_workspace_lockfile
                    && let Some(workspace_dir) = cfg.workspace_dir.clone()
                {
                    let cfg: &'static Config = cfg;
                    return run_dedicated_lockfile_workspace_install::<Reporter>(
                        &args,
                        cfg,
                        &workspace_dir,
                        require_lockfile,
                    )
                    .await;
                }
                let cfg: &'static Config = cfg;
                let state = State::init(manifest_path, cfg, require_lockfile)
                    .wrap_err("initialize the state")?;
                Box::pin(args.run::<Reporter>(state)).await
            }
        }
    }
}

pub(crate) struct AddPipeline {
    pub(crate) args: AddArgs,
    pub(crate) cfg: &'static mut Config,
    pub(crate) config_root: PathBuf,
    pub(crate) package_manager_to_sync: Option<PackageManagerToSync>,
    pub(crate) prefix: PathBuf,
    pub(crate) manifest_path: PathBuf,
    pub(crate) recursive_sort: bool,
    /// [`AddArgs::parse_config_dependencies`]'s output, parsed by the dispatch
    /// before this pipeline scaffolds a manifest. `Some` exactly when
    /// `--config` was passed.
    pub(crate) config_dependencies: Option<BTreeMap<String, String>>,
}

impl AddPipeline {
    pub(crate) async fn run<Reporter: self::Reporter + 'static>(self) -> miette::Result<()> {
        let AddPipeline {
            args,
            cfg,
            config_root,
            package_manager_to_sync,
            prefix,
            manifest_path,
            recursive_sort,
            config_dependencies,
        } = self;
        if let Some(pm) = package_manager_to_sync.as_ref() {
            config_deps::sync_package_manager_dependencies(
                cfg,
                &config_root,
                &pm.specifier,
                &pm.version,
                false,
            )
            .await?;
        }
        config_deps::install_config_deps::<Reporter>(cfg, &config_root, false).await?;
        config_deps::run_update_config_hooks::<Reporter>(cfg, &config_root).await?;
        // `--config` targets the workspace's configuration dependencies, not
        // any project's manifest, so it bypasses project selection entirely.
        let plan = if config_dependencies.is_some() {
            InstallFamilyPlan::Single
        } else {
            select_install_family_plan(cfg, &prefix, &manifest_path, recursive_sort, true)?
        };
        match plan {
            InstallFamilyPlan::PerProject(project_dirs) => {
                // Dedicated per-project lockfiles: add the packages to each
                // selected project independently.
                let cfg: &Config = cfg;
                for project_dir in project_dirs {
                    let state = init_dedicated_project_state(cfg, &project_dir, false)?;
                    Box::pin(args.clone().run::<Reporter>(state, None)).await?;
                }
                Ok(())
            }
            InstallFamilyPlan::Shared(selection) => {
                if selection.selected_dirs.is_empty() {
                    return Ok(());
                }
                let cfg: &'static Config = cfg;
                let state =
                    State::init(manifest_path, cfg, false).wrap_err("initialize the state")?;
                Box::pin(args.run_selected::<Reporter>(state, *selection)).await
            }
            InstallFamilyPlan::Single => {
                // Dedicated per-project lockfiles: `add` mutates only the
                // active project, whose outputs anchor at the project dir.
                // `--config` targets the workspace's configuration
                // dependencies, which stay workspace-anchored.
                if config_dependencies.is_none()
                    && !cfg.shared_workspace_lockfile
                    && cfg.workspace_dir.is_some()
                {
                    let manifest_dir = manifest_path
                        .parent()
                        .expect("manifest path always has a parent dir")
                        .to_path_buf();
                    anchor_dedicated_project_config(cfg, &manifest_dir);
                }
                let cfg: &'static Config = cfg;
                let state =
                    State::init(manifest_path, cfg, false).wrap_err("initialize the state")?;
                Box::pin(args.run::<Reporter>(state, config_dependencies)).await
            }
        }
    }
}

pub(crate) struct UpdatePipeline {
    pub(crate) args: UpdateArgs,
    pub(crate) cfg: &'static mut Config,
    pub(crate) config_root: PathBuf,
    pub(crate) package_manager_to_sync: Option<PackageManagerToSync>,
    pub(crate) prefix: PathBuf,
    pub(crate) manifest_path: PathBuf,
    pub(crate) recursive_sort: bool,
}

impl UpdatePipeline {
    pub(crate) async fn run<Reporter: self::Reporter + 'static>(self) -> miette::Result<()> {
        let UpdatePipeline {
            args,
            cfg,
            config_root,
            package_manager_to_sync,
            prefix,
            manifest_path,
            recursive_sort,
        } = self;
        if let Some(pm) = package_manager_to_sync.as_ref() {
            config_deps::sync_package_manager_dependencies(
                cfg,
                &config_root,
                &pm.specifier,
                &pm.version,
                false,
            )
            .await?;
        }
        config_deps::install_config_deps::<Reporter>(cfg, &config_root, false).await?;
        config_deps::run_update_config_hooks::<Reporter>(cfg, &config_root).await?;
        let plan = select_install_family_plan(cfg, &prefix, &manifest_path, recursive_sort, false)?;
        // An empty selection has nothing to update, and — like the shared
        // path — must not generate a changeset.
        match &plan {
            InstallFamilyPlan::PerProject(project_dirs) if project_dirs.is_empty() => {
                return Ok(());
            }
            InstallFamilyPlan::Shared(selection) if selection.selected_dirs.is_empty() => {
                return Ok(());
            }
            _ => {}
        }
        // Dedicated per-project lockfiles: the non-recursive command
        // mutates only the active project, whose outputs anchor at the
        // project dir.
        if matches!(plan, InstallFamilyPlan::Single)
            && !cfg.shared_workspace_lockfile
            && cfg.workspace_dir.is_some()
        {
            let manifest_dir = manifest_path
                .parent()
                .expect("manifest path always has a parent dir")
                .to_path_buf();
            anchor_dedicated_project_config(cfg, &manifest_dir);
        }
        let generate_changeset = if args.changeset {
            true
        } else if args.no_changeset {
            false
        } else {
            cfg.update_config.changeset.unwrap_or(false)
        };
        let changeset_context = generate_changeset
            .then(|| UpdateChangesetContext::capture(cfg, &manifest_path))
            .transpose()?;
        match plan {
            InstallFamilyPlan::PerProject(project_dirs) => {
                let cfg: &Config = cfg;
                for project_dir in project_dirs {
                    let state = init_dedicated_project_state(cfg, &project_dir, false)?;
                    Box::pin(args.clone().run::<Reporter>(state)).await?;
                }
            }
            InstallFamilyPlan::Shared(selection) => {
                let cfg: &'static Config = cfg;
                let state =
                    State::init(manifest_path, cfg, false).wrap_err("initialize the state")?;
                Box::pin(args.run_selected::<Reporter>(state, *selection)).await?;
            }
            InstallFamilyPlan::Single => {
                let cfg: &'static Config = cfg;
                let state =
                    State::init(manifest_path, cfg, false).wrap_err("initialize the state")?;
                Box::pin(args.run::<Reporter>(state)).await?;
            }
        }
        if let Some(changeset_context) = changeset_context {
            changeset_context.generate::<Reporter>()?;
        }
        Ok(())
    }
}

pub(crate) struct RemovePipeline {
    pub(crate) args: RemoveArgs,
    pub(crate) cfg: &'static mut Config,
    pub(crate) config_root: PathBuf,
    pub(crate) package_manager_to_sync: Option<PackageManagerToSync>,
    pub(crate) prefix: PathBuf,
    pub(crate) manifest_path: PathBuf,
    pub(crate) recursive_sort: bool,
}

impl RemovePipeline {
    pub(crate) async fn run<Reporter: self::Reporter + 'static>(self) -> miette::Result<()> {
        let RemovePipeline {
            args,
            cfg,
            config_root,
            package_manager_to_sync,
            prefix,
            manifest_path,
            recursive_sort,
        } = self;
        if let Some(pm) = package_manager_to_sync.as_ref() {
            config_deps::sync_package_manager_dependencies(
                cfg,
                &config_root,
                &pm.specifier,
                &pm.version,
                false,
            )
            .await?;
        }
        config_deps::install_config_deps::<Reporter>(cfg, &config_root, false).await?;
        config_deps::run_update_config_hooks::<Reporter>(cfg, &config_root).await?;
        let plan = select_install_family_plan(cfg, &prefix, &manifest_path, recursive_sort, false)?;
        match plan {
            InstallFamilyPlan::PerProject(project_dirs) => {
                // Dedicated per-project lockfiles: remove the packages from
                // each selected project independently.
                let cfg: &Config = cfg;
                for project_dir in project_dirs {
                    let state = init_dedicated_project_state(cfg, &project_dir, false)?;
                    Box::pin(args.clone().run::<Reporter>(state)).await?;
                }
                Ok(())
            }
            InstallFamilyPlan::Shared(selection) => {
                if selection.selected_dirs.is_empty() {
                    return Ok(());
                }
                let cfg: &'static Config = cfg;
                let state =
                    State::init(manifest_path, cfg, false).wrap_err("initialize the state")?;
                Box::pin(args.run_selected::<Reporter>(state, *selection)).await
            }
            InstallFamilyPlan::Single => {
                // Dedicated per-project lockfiles: the non-recursive command
                // mutates only the active project, whose outputs anchor at the
                // project dir.
                if !cfg.shared_workspace_lockfile && cfg.workspace_dir.is_some() {
                    let manifest_dir = manifest_path
                        .parent()
                        .expect("manifest path always has a parent dir")
                        .to_path_buf();
                    anchor_dedicated_project_config(cfg, &manifest_dir);
                }
                let cfg: &'static Config = cfg;
                let state =
                    State::init(manifest_path, cfg, false).wrap_err("initialize the state")?;
                Box::pin(args.run::<Reporter>(state)).await
            }
        }
    }
}

pub(crate) struct DeployPipeline {
    pub(crate) args: DeployArgs,
    pub(crate) cfg: &'static mut Config,
    pub(crate) config_root: PathBuf,
    pub(crate) package_manager_to_sync: Option<PackageManagerToSync>,
}

impl DeployPipeline {
    pub(crate) async fn run<Reporter: self::Reporter + 'static>(
        self,
        dir_ref: &Path,
    ) -> miette::Result<()> {
        let DeployPipeline { args, cfg, config_root, package_manager_to_sync } = self;
        if let Some(pm) = package_manager_to_sync.as_ref() {
            config_deps::sync_package_manager_dependencies(
                cfg,
                &config_root,
                &pm.specifier,
                &pm.version,
                false,
            )
            .await?;
        }
        config_deps::install_config_deps::<Reporter>(cfg, &config_root, false).await?;
        config_deps::run_update_config_hooks::<Reporter>(cfg, &config_root).await?;
        let cfg: &'static Config = cfg;
        Box::pin(args.run::<Reporter>(cfg, dir_ref)).await
    }
}

/// Re-anchor the per-project output paths for dedicated per-project
/// lockfiles (`sharedWorkspaceLockfile: false`): `node_modules` and the
/// virtual store live under the project, mirroring pnpm, which resolves
/// them against `lockfileDir` — the project dir in dedicated mode. An
/// explicit `virtualStoreDir` setting re-resolves against the project
/// (its raw value is recovered from [`Config::explicit_settings`]);
/// the default stays `<modules_dir>/.pnpm`. Global-virtual-store
/// installs keep their store-anchored `virtual_store_dir`.
pub(crate) fn anchor_dedicated_project_config(config: &mut Config, project_dir: &Path) {
    // Both re-anchored paths resolve the *raw* setting (recovered from
    // [`Config::explicit_settings`]) against the project dir, so a
    // multi-component or absolute value keeps its full shape —
    // `Path::join` keeps an absolute setting absolute.
    config.modules_dir =
        match config.explicit_settings.get("modulesDir").and_then(serde_json::Value::as_str) {
            Some(raw) => project_dir.join(raw),
            None => project_dir.join("node_modules"),
        };
    if !config.enable_global_virtual_store {
        config.virtual_store_dir = match config
            .explicit_settings
            .get("virtualStoreDir")
            .and_then(serde_json::Value::as_str)
        {
            Some(raw) => project_dir.join(raw),
            None => config.modules_dir.join(".pnpm"),
        };
    }
}

/// `sharedWorkspaceLockfile: false` workspace install: one independent
/// single-project install per workspace project — each gets its own
/// `pnpm-lock.yaml`, `node_modules`, and virtual store, mirroring
/// pnpm's dedicated-lockfile per-project loop in its recursive
/// dispatch. The workspace root participates when it has a manifest,
/// matching the project set a shared-lockfile workspace install covers.
async fn run_dedicated_lockfile_workspace_install<Reporter: self::Reporter + 'static>(
    args: &super::install::InstallArgs,
    cfg: &Config,
    workspace_root: &Path,
    require_lockfile: bool,
) -> miette::Result<()> {
    let (projects, _patterns) = discover_workspace_projects(workspace_root)?;
    let normalized_root = pacquet_fs::lexical_normalize(workspace_root);
    let mut project_dirs: Vec<PathBuf> = Vec::with_capacity(projects.len() + 1);
    if workspace_root.join("package.json").is_file()
        && !projects
            .iter()
            .any(|project| pacquet_fs::lexical_normalize(&project.root_dir) == normalized_root)
    {
        project_dirs.push(workspace_root.to_path_buf());
    }
    project_dirs.extend(projects.into_iter().map(|project| project.root_dir));
    // One `Config::leak` per project: `State::init` needs a
    // `&'static Config`, and a leaked shared reference can't be
    // reclaimed for the next iteration. The leak is bounded by the
    // project count, happens once per CLI invocation, and is
    // reclaimed at process exit — the same lifetime deploy's derived
    // install config has.
    for project_dir in project_dirs {
        let state = init_dedicated_project_state(cfg, &project_dir, require_lockfile)?;
        Box::pin(args.clone().run::<Reporter>(state)).await?;
    }
    Ok(())
}

/// Shared workspace-root and package-manager policy derivation used by the
/// install, dedupe, and prune dispatch paths.
pub(crate) fn derive_config_root_and_package_manager_to_sync(
    cfg: &Config,
    dir_ref: &Path,
) -> miette::Result<(PathBuf, Option<PackageManagerToSync>)> {
    let config_root = cfg.workspace_dir.clone().unwrap_or_else(|| dir_ref.to_path_buf());
    let package_manager_to_sync =
        package_manager_to_sync(&config_root.join("package.json"), &config_root)
            .wrap_err("read package manager policy")?;
    Ok((config_root, package_manager_to_sync))
}

pub(crate) fn apply_install_cli_config(cfg: &mut Config, args: &InstallArgs) {
    cfg.offline = resolve_bool_override(args.offline, args.no_offline, cfg.offline);
    cfg.prefer_offline =
        resolve_bool_override(args.prefer_offline, args.no_prefer_offline, cfg.prefer_offline);
    cfg.frozen_store =
        resolve_bool_override(args.frozen_store, args.no_frozen_store, cfg.frozen_store);
    cfg.ignore_scripts =
        resolve_bool_override(args.ignore_scripts, args.no_ignore_scripts, cfg.ignore_scripts);
    cfg.force = args.force || cfg.force;
    if let Some(network_concurrency) = args.network_concurrency {
        cfg.network_concurrency = network_concurrency;
    }
    if let Some(fetch_timeout) = args.fetch_timeout {
        cfg.fetch_timeout = fetch_timeout;
    }
    if let Some(user_agent) = args.user_agent.clone() {
        cfg.user_agent = user_agent;
    }
    if let Some(pnpr_server) = args.pnpr_server.clone() {
        cfg.pnpr_server = Some(pnpr_server);
    }
}

/// The reporter-generic body of `pacquet dedupe`: snapshots the lockfile
/// (when `--check`), runs config-dependency installation and `updateConfig`
/// hooks, then dispatches to the install pipeline. The snapshot wraps the
/// entire pipeline so any lockfile write made by config-deps is also covered
/// by the check gate.
pub(crate) struct DedupePipeline {
    pub(crate) args: DedupeArgs,
    pub(crate) cfg: &'static mut Config,
    pub(crate) config_root: PathBuf,
    pub(crate) package_manager_to_sync: Option<PackageManagerToSync>,
    pub(crate) manifest_path: PathBuf,
}

impl DedupePipeline {
    pub(crate) async fn run<Reporter: self::Reporter + 'static>(self) -> miette::Result<()> {
        let DedupePipeline { args, cfg, config_root, package_manager_to_sync, manifest_path } =
            self;

        let lockfile_path = config_root.join(pacquet_lockfile::Lockfile::FILE_NAME);

        // Snapshot before any config-dep writes so --check detects lockfile
        // changes made by config-dependency syncing as well.
        let existing =
            if args.check { dedupe::read_lockfile_snapshot(&lockfile_path)? } else { None };
        let guard =
            args.check.then(|| dedupe::LockfileGuard::new(existing.clone(), &lockfile_path));

        if let Some(pm) = package_manager_to_sync.as_ref() {
            config_deps::sync_package_manager_dependencies(
                cfg,
                &config_root,
                &pm.specifier,
                &pm.version,
                false,
            )
            .await?;
        }
        config_deps::install_config_deps::<Reporter>(cfg, &config_root, false).await?;
        config_deps::run_update_config_hooks::<Reporter>(cfg, &config_root).await?;
        let cfg: &'static Config = cfg;
        let state = State::init(manifest_path, cfg, false).wrap_err("initialize the state")?;
        Box::pin(args.run::<Reporter>(state, existing, guard, &lockfile_path)).await
    }
}

/// The reporter-generic body of `pacquet prune`: runs config-deps and
/// `updateConfig` hooks first, then applies prune-specific config
/// overrides (`modules_cache_max_age`, `ignore_scripts`) on the
/// post-hook config, and finally dispatches to the install pipeline.
/// The overrides must come after hooks because `updateConfig` can
/// mutate `Config` fields (including `modules_dir` /
/// `virtual_store_dir`), and the CLI `--ignore-scripts` flag must win
/// over any hook-set value.
pub(crate) struct PrunePipeline {
    pub(crate) args: PruneArgs,
    pub(crate) cfg: &'static mut Config,
    pub(crate) config_root: PathBuf,
    pub(crate) package_manager_to_sync: Option<PackageManagerToSync>,
    pub(crate) manifest_path: PathBuf,
}

impl PrunePipeline {
    pub(crate) async fn run<Reporter: self::Reporter + 'static>(self) -> miette::Result<()> {
        let PrunePipeline { args, cfg, config_root, package_manager_to_sync, manifest_path } = self;

        if let Some(pm) = package_manager_to_sync.as_ref() {
            config_deps::sync_package_manager_dependencies(
                cfg,
                &config_root,
                &pm.specifier,
                &pm.version,
                false,
            )
            .await?;
        }
        config_deps::install_config_deps::<Reporter>(cfg, &config_root, false).await?;
        config_deps::run_update_config_hooks::<Reporter>(cfg, &config_root).await?;
        // Validate path containment AFTER hooks: updateConfig can mutate
        // modules_dir / virtual_store_dir via WorkspaceSettings::apply_to,
        // so the check must use the final (post-hook) config values.
        // The install pipeline's prune_target_within_modules also validates
        // VSD containment, but only at sweep time; this earlier check
        // catches a misconfigured modules_dir itself (e.g. an absolute
        // path outside the workspace) before any destructive work begins.
        //
        // `config_root` is `cfg.workspace_dir` when present, or the
        // canonicalized `--dir` otherwise — a meaningful containment
        // boundary in both cases.
        if !cfg.modules_dir.starts_with(&config_root) {
            let modules_dir = cfg.modules_dir.display();
            let cr = config_root.display();
            return Err(miette::miette!(
                "refusing prune: modules_dir ({modules_dir}) is outside workspace root ({cr})",
            ));
        }
        // Apply prune-specific overrides after hooks so that:
        // - `modules_cache_max_age = 0` forces the virtual-store sweep
        //   on the final (post-hook) config paths.
        // - `--ignore-scripts` from the CLI wins over any value the
        //   hooks set via `WorkspaceSettings::apply_to`.
        cfg.modules_cache_max_age = 0;
        cfg.ignore_scripts =
            resolve_bool_override(args.ignore_scripts, args.no_ignore_scripts, cfg.ignore_scripts);
        let cfg: &'static Config = cfg;
        let state = State::init(manifest_path, cfg, false).wrap_err("initialize the state")?;
        Box::pin(args.run::<Reporter>(state)).await
    }
}
