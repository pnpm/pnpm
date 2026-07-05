use super::{
    dedupe::{self, DedupeArgs},
    deploy::DeployArgs,
    install::{InstallArgs, resolve_bool_override},
    package_manager::{PackageManagerToSync, package_manager_to_sync},
    prune::PruneArgs,
};
use crate::{State, config_deps};
use miette::Context;
use pacquet_config::Config;
use pacquet_reporter::Reporter;
use std::path::{Path, PathBuf};

/// The reporter-generic body of `pacquet install`: it threads one `Reporter`
/// type through config-dependency sync, the `updateConfig` hooks, and the
/// install itself. Lifting it out of the dispatch keeps the three
/// `ReporterType` arms to a single line each.
pub(crate) struct InstallPipeline {
    pub(crate) args: InstallArgs,
    pub(crate) cfg: &'static mut Config,
    pub(crate) config_root: PathBuf,
    pub(crate) package_manager_to_sync: Option<PackageManagerToSync>,
    pub(crate) manifest_path: PathBuf,
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
            manifest_path,
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
        let cfg: &'static Config = cfg;
        let state =
            State::init(manifest_path, cfg, require_lockfile).wrap_err("initialize the state")?;
        Box::pin(args.run::<Reporter>(state)).await
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
    cfg.workspace_concurrency = args.resolve_workspace_concurrency(cfg.workspace_concurrency);
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
        args.run::<Reporter>(state, existing, guard, &lockfile_path).await
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
        args.run::<Reporter>(state).await
    }
}
