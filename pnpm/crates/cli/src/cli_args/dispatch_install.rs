pub(super) use maintenance::{
    approve_builds, dedupe, deploy, env, fetch, import, link, prune, rebuild, runtime, unlink,
};
pub(super) use patches::{patch, patch_commit, patch_remove};
pub(super) use pipeline::{install_test, pipeline};

use super::{
    add::{AddArgs, apply_allow_build},
    approve_builds::ApproveBuildsArgs,
    ci::CiArgs,
    create::CreateArgs,
    dedupe::DedupeArgs,
    deploy::DeployArgs,
    dispatch::{CommandFuture, RunCtx, apply_update_config},
    dlx::DlxArgs,
    env::{EnvArgs, EnvSubcommand},
    fetch::FetchArgs,
    global,
    import::ImportArgs,
    install::{InstallArgs, resolve_bool_override},
    link::LinkArgs,
    patch::PatchArgs,
    patch_commit::PatchCommitArgs,
    patch_remove::PatchRemoveArgs,
    pipeline::{PipelineArgs, PipelineInvocation, WatchInvocation, run_pipeline, run_watch},
    pipelines::{
        AddPipeline, DedupePipeline, DeployPipeline, InstallPipeline, PrunePipeline,
        RemovePipeline, UpdatePipeline, apply_install_cli_config, derive_config_root,
    },
    prune::PruneArgs,
    rebuild::RebuildArgs,
    remove::RemoveArgs,
    reporter::{ReporterType, reporter_emit},
    runtime::RuntimeArgs,
    unlink::UnlinkArgs,
    update::UpdateArgs,
    update_notifier,
    workspace_option::workspace_link_root,
};
use crate::{State, package_specifier::PackageSpecifierPlan};

use miette::Context;

use pnpm_config::Config;
use pnpm_default_reporter::DefaultReporter;
use pnpm_reporter::{NdjsonReporter, SilentReporter};
use std::path::{Path, PathBuf};

pub(super) fn add<'a>(ctx: &RunCtx<'a>, args: AddArgs) -> miette::Result<CommandFuture<'a>> {
    let package_specifier_plan = PackageSpecifierPlan::parse(&args.package_names)?;
    check_specifier_combination(&args, &package_specifier_plan)?;
    if args.global {
        return add_global(ctx, args);
    }
    let config_dependencies = args.parse_config_dependencies()?;
    let dir = ctx.dir;
    let manifest_path = ctx.manifest_path;
    let reporter = ctx.reporter;
    let config = ctx.config;
    Ok(Box::pin(async move {
        let cfg = config()?;
        let (config_root, recursive_sort) =
            prepare_add_config(&args, cfg, dir, reporter, config_dependencies.is_none())?;
        let update_check = update_notifier::spawn(cfg, reporter_emit(reporter));
        let pipeline = AddPipeline {
            args,
            cfg,
            config_root,
            prefix: dir.to_path_buf(),
            manifest_path: manifest_path.to_path_buf(),
            recursive_sort,
            config_dependencies,
            package_specifier_plan,
        };
        let added = match reporter {
            ReporterType::Default | ReporterType::AppendOnly => {
                Box::pin(pipeline.run::<DefaultReporter>()).await
            }
            ReporterType::Ndjson => Box::pin(pipeline.run::<NdjsonReporter>()).await,
            ReporterType::Silent => Box::pin(pipeline.run::<SilentReporter>()).await,
        };
        update_notifier::settle(update_check, &added).await;
        added
    }))
}

/// Apply the add's settings to the config and derive its root. Returns the
/// config root and the `sort` setting as it stood before the command-line
/// settings applied.
fn prepare_add_config(
    args: &AddArgs,
    cfg: &mut Config,
    dir: &Path,
    reporter: ReporterType,
    plain_dependencies: bool,
) -> miette::Result<(PathBuf, bool)> {
    // Before `apply_allow_build` persists anything: a `--workspace` add
    // that cannot run must leave `pnpm-workspace.yaml` untouched.
    workspace_link_root(args.workspace, cfg.workspace_dir.as_deref())?;
    let recursive_sort = cfg.sort;
    if plain_dependencies {
        args.check_workspace_root(cfg, dir)?;
    }
    args.lockfile_dir.apply_to(cfg, dir);
    args.apply_cli_config(cfg);
    let config_root = derive_config_root(cfg, dir, reporter)
        .wrap_err("derive workspace root and package manager policy")?;
    // `allowBuilds` is persisted to `pnpm-workspace.yaml`, which stays
    // at the workspace root even when `lockfileDir` moved the config
    // root elsewhere.
    let allow_build_root = cfg.workspace_dir.clone().unwrap_or_else(|| config_root.clone());
    apply_allow_build(cfg, &args.allow_build, &allow_build_root)?;
    Ok((config_root, recursive_sort))
}

fn add_global<'a>(ctx: &RunCtx<'a>, args: AddArgs) -> miette::Result<CommandFuture<'a>> {
    let config = (ctx.global_config)()?;
    args.lockfile_dir.apply_to_global(config)?;
    args.apply_cli_config(config);
    let dir = ctx.dir;
    let update_check = update_notifier::spawn(config, reporter_emit(ctx.reporter));
    let install: CommandFuture<'a> = match ctx.reporter {
        ReporterType::Default | ReporterType::AppendOnly => {
            Box::pin(args.run_global::<DefaultReporter>(config, dir))
        }
        ReporterType::Ndjson => Box::pin(args.run_global::<NdjsonReporter>(config, dir)),
        ReporterType::Silent => Box::pin(args.run_global::<SilentReporter>(config, dir)),
    };
    Ok(Box::pin(async move {
        let installed = install.await;
        update_notifier::settle(update_check, &installed).await;
        installed
    }))
}

/// Reject the flag and specifier combinations `pnpm add` cannot honour.
///
/// Checked up front: `AddPipeline::run` scaffolds a `package.json`
/// through `State::init`, and an invalid selector must be rejected
/// before that.
fn check_specifier_combination(args: &AddArgs, plan: &PackageSpecifierPlan) -> miette::Result<()> {
    if args.dependency_options.save_build() && !plan.has_cargo() {
        return Err(miette::miette!("--save-build requires at least one crate: dependency"));
    }
    if args.workspace && (plan.has_cargo() || plan.has_python()) {
        return Err(miette::miette!(
            "--workspace cannot be combined with crate: or pypi: dependencies"
        ));
    }
    if args.workspace && args.config {
        return Err(miette::miette!("`pnpm add --config` cannot be combined with --workspace."));
    }
    check_non_npm_targets(args, plan)
}

/// A `crate:` or `pypi:` specifier has no global install and no
/// configuration-dependency form.
fn check_non_npm_targets(args: &AddArgs, plan: &PackageSpecifierPlan) -> miette::Result<()> {
    if args.global {
        if plan.has_cargo() {
            return Err(miette::miette!("crate: dependencies cannot be installed globally"));
        }
        if plan.has_python() {
            return Err(miette::miette!("pypi: dependencies cannot be installed globally"));
        }
    }
    if args.config {
        if plan.has_cargo() {
            return Err(miette::miette!(
                "crate: dependencies cannot be configuration dependencies"
            ));
        }
        if plan.has_python() {
            return Err(miette::miette!("pypi: dependencies cannot be configuration dependencies"));
        }
    }
    Ok(())
}

pub(super) fn update<'a>(ctx: &RunCtx<'a>, args: UpdateArgs) -> miette::Result<CommandFuture<'a>> {
    if args.global {
        let config = (ctx.global_config)()?;
        args.lockfile_dir.apply_to_global(config)?;
        args.apply_cli_config(config);
        return Ok(match ctx.reporter {
            ReporterType::Default | ReporterType::AppendOnly => {
                Box::pin(args.run_global::<DefaultReporter>(config))
            }
            ReporterType::Ndjson => Box::pin(args.run_global::<NdjsonReporter>(config)),
            ReporterType::Silent => Box::pin(args.run_global::<SilentReporter>(config)),
        });
    }
    let dir = ctx.dir;
    let manifest_path = ctx.manifest_path;
    let reporter = ctx.reporter;
    let config = ctx.config;
    Ok(Box::pin(async move {
        let cfg = config()?;
        let recursive_sort = cfg.sort;
        args.lockfile_dir.apply_to(cfg, dir);
        args.apply_cli_config(cfg);
        let config_root = derive_config_root(cfg, dir, reporter)
            .wrap_err("derive workspace root and package manager policy")?;
        let pipeline = UpdatePipeline {
            args,
            cfg,
            config_root,
            prefix: dir.to_path_buf(),
            manifest_path: manifest_path.to_path_buf(),
            recursive_sort,
        };
        match reporter {
            ReporterType::Default | ReporterType::AppendOnly => {
                Box::pin(pipeline.run::<DefaultReporter>()).await?;
            }
            ReporterType::Ndjson => Box::pin(pipeline.run::<NdjsonReporter>()).await?,
            ReporterType::Silent => Box::pin(pipeline.run::<SilentReporter>()).await?,
        }
        Ok(())
    }))
}

pub(super) fn remove<'a>(ctx: &RunCtx<'a>, args: RemoveArgs) -> miette::Result<CommandFuture<'a>> {
    if args.global {
        remove_global(ctx, &args)?;
        return Ok(Box::pin(std::future::ready(Ok(()))));
    }
    let dir = ctx.dir;
    let manifest_path = ctx.manifest_path;
    let reporter = ctx.reporter;
    let config = ctx.config;
    Ok(Box::pin(async move {
        let cfg = config()?;
        let recursive_sort = cfg.sort;
        args.lockfile_dir.apply_to(cfg, dir);
        let config_root = derive_config_root(cfg, dir, reporter)
            .wrap_err("derive workspace root and package manager policy")?;
        let pipeline = RemovePipeline {
            args,
            cfg,
            config_root,
            prefix: dir.to_path_buf(),
            manifest_path: manifest_path.to_path_buf(),
            recursive_sort,
        };
        match reporter {
            ReporterType::Default | ReporterType::AppendOnly => {
                Box::pin(pipeline.run::<DefaultReporter>()).await?;
            }
            ReporterType::Ndjson => Box::pin(pipeline.run::<NdjsonReporter>()).await?,
            ReporterType::Silent => Box::pin(pipeline.run::<SilentReporter>()).await?,
        }
        Ok(())
    }))
}

/// Whether the command driving the install pipeline is one pnpm checks for
/// a newer pnpm on. `pnpm ci` and `pnpm install-test` run the same pipeline
/// but are their own commands, and pnpm only checks on `install` and `add`.
#[derive(Debug, Clone, Copy)]
enum UpdateCheckPolicy {
    Run,
    Skip,
}

pub(super) fn install<'a>(
    ctx: &RunCtx<'a>,
    args: InstallArgs,
) -> miette::Result<CommandFuture<'a>> {
    install_with_update_check(ctx, args, UpdateCheckPolicy::Run)
}

fn install_with_update_check<'a>(
    ctx: &RunCtx<'a>,
    args: InstallArgs,
    update_check_policy: UpdateCheckPolicy,
) -> miette::Result<CommandFuture<'a>> {
    let install = install_with_config(ctx, args, update_check_policy)?;
    Ok(Box::pin(async move { install.await.map(|_| ()) }))
}

fn install_with_config<'a>(
    ctx: &RunCtx<'a>,
    args: InstallArgs,
    update_check_policy: UpdateCheckPolicy,
) -> miette::Result<CommandFuture<'a, &'static Config>> {
    let dir = ctx.dir;
    let manifest_path = ctx.manifest_path;
    let reporter = ctx.reporter;
    let config = ctx.config;
    Ok(Box::pin(async move {
        // Boxed for `clippy::large_stack_frames`: the three
        // monomorphized install futures would otherwise each reserve
        // their full size in this frame.
        {
            // Applied between `config()` and `State::init`, while
            // the loaded `Config` is still mutable through
            // `Config::leak`'s `&'static mut Config` return. How
            // each `--flag` / `--no-flag` pair beats the configured
            // value is `resolve_bool_override`'s contract.
            let cfg = config()?;
            let recursive_sort = cfg.sort;
            args.lockfile_dir.apply_to(cfg, dir);
            apply_install_cli_config(cfg, &args);
            let frozen_lockfile = args.effective_frozen_lockfile(cfg);
            let require_lockfile = frozen_lockfile;
            // Config dependencies are workspace-level state: their
            // `.pnpm-config` and env lockfile live at the lockfile /
            // workspace root, not the CLI cwd. Use the same root
            // `State::init` uses (`config.workspace_dir`, set when a
            // `pnpm-workspace.yaml` is found), falling back to `--dir`
            // for a single-package repo. Owned so it doesn't hold a
            // borrow of `cfg` across the `&mut` `updateConfig` pass.
            let config_root = derive_config_root(cfg, dir, reporter)
                .wrap_err("derive workspace root and package manager policy")?;
            let update_check = match update_check_policy {
                UpdateCheckPolicy::Run => update_notifier::spawn(cfg, reporter_emit(reporter)),
                UpdateCheckPolicy::Skip => None,
            };
            // Resolve + install configurational dependencies, then
            // run their `updateConfig` plugin hooks, before the main
            // install. The env lockfile must land at the top of
            // `pnpm-lock.yaml` before `State::init` loads the wanted
            // lockfile, and `updateConfig` must mutate `cfg` (still
            // `&'static mut`) before it's frozen and the install
            // reads it. Mirrors pnpm running both at
            // config-finalization.
            let pipeline = InstallPipeline {
                args,
                cfg,
                config_root,
                prefix: dir.to_path_buf(),
                manifest_path: manifest_path.to_path_buf(),
                recursive_sort,
                require_lockfile,
                frozen_lockfile,
            };
            let installed = run_install_pipeline(pipeline, reporter).await;
            update_notifier::settle(update_check, &installed).await;
            installed
        }
    }))
}

pub(super) fn ci<'a>(ctx: &RunCtx<'a>, args: CiArgs) -> miette::Result<CommandFuture<'a>> {
    let clean_args = args.clean_args;
    let mut install_args = args.install_args;
    install_args.frozen_lockfile = true;

    // Run clean eagerly before the async future so errors surface immediately. Pass the command name so a package.json script can override the built-in.
    clean_args.run(ctx, "clean")?;

    install_with_update_check(ctx, install_args, UpdateCheckPolicy::Skip)
}

pub(super) fn dlx<'a>(ctx: &RunCtx<'a>, args: DlxArgs) -> miette::Result<CommandFuture<'a>> {
    let dir = ctx.dir;
    Ok(match ctx.reporter {
        ReporterType::Default | ReporterType::AppendOnly => {
            Box::pin(args.run::<DefaultReporter>(dir, (ctx.config)()?))
        }
        ReporterType::Ndjson => Box::pin(args.run::<NdjsonReporter>(dir, (ctx.config)()?)),
        ReporterType::Silent => Box::pin(args.run::<SilentReporter>(dir, (ctx.config)()?)),
    })
}

pub(super) fn create<'a>(ctx: &RunCtx<'a>, args: CreateArgs) -> miette::Result<CommandFuture<'a>> {
    let dir = ctx.dir;
    Ok(match ctx.reporter {
        ReporterType::Default | ReporterType::AppendOnly => {
            Box::pin(args.run::<DefaultReporter>(dir, (ctx.config)()?))
        }
        ReporterType::Ndjson => Box::pin(args.run::<NdjsonReporter>(dir, (ctx.config)()?)),
        ReporterType::Silent => Box::pin(args.run::<SilentReporter>(dir, (ctx.config)()?)),
    })
}

fn remove_global(ctx: &RunCtx<'_>, args: &RemoveArgs) -> miette::Result<()> {
    let config = (ctx.global_config)()?;
    args.lockfile_dir.apply_to_global(config)?;
    match ctx.reporter {
        ReporterType::Default | ReporterType::AppendOnly => {
            global::handle_global_remove::<DefaultReporter>(config, &args.package_names)?;
        }
        ReporterType::Ndjson => {
            global::handle_global_remove::<NdjsonReporter>(config, &args.package_names)?;
        }
        ReporterType::Silent => {
            global::handle_global_remove::<SilentReporter>(config, &args.package_names)?;
        }
    }
    Ok(())
}

async fn run_install_pipeline(
    pipeline: InstallPipeline,
    reporter: ReporterType,
) -> miette::Result<&'static Config> {
    match reporter {
        ReporterType::Default | ReporterType::AppendOnly => {
            Box::pin(pipeline.run_with_config::<DefaultReporter>()).await
        }
        ReporterType::Ndjson => Box::pin(pipeline.run_with_config::<NdjsonReporter>()).await,
        ReporterType::Silent => Box::pin(pipeline.run_with_config::<SilentReporter>()).await,
    }
}

mod maintenance;

mod pipeline;

mod patches;
