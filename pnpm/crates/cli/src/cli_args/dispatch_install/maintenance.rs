use super::{
    super::{dispatch_script, rebuild, script_override},
    ApproveBuildsArgs, CommandFuture, Config, Context, DedupeArgs, DedupePipeline, DefaultReporter,
    DeployArgs, DeployPipeline, EnvArgs, EnvSubcommand, FetchArgs, ImportArgs, InstallArgs,
    InstallPipeline, LinkArgs, NdjsonReporter, Path, PruneArgs, PrunePipeline, RebuildArgs,
    ReporterType, RunCtx, RuntimeArgs, SilentReporter, UnlinkArgs, apply_install_cli_config,
    apply_update_config, derive_config_root, global, resolve_bool_override,
};

pub(in super::super) fn deploy<'a>(
    ctx: &RunCtx<'a>,
    args: DeployArgs,
) -> miette::Result<CommandFuture<'a>> {
    let dir = ctx.locations.dir;
    let reporter = ctx.reporter();
    let config = ctx.loaders.config;
    let cfg = config()?;
    // Ahead of the target validation, so a project with a `deploy` script
    // never has to name a target it does not deploy to.
    let script_args = args.target_dirs
        .iter()
        .map(|target| target.to_string_lossy().into_owned())
        .collect();
    if let Some(run_args) = script_override::resolve(ctx, cfg, "deploy", script_args)? {
        return dispatch_script::run(ctx, run_args);
    }
    apply_install_cli_config(cfg, &args.install_args);
    Ok(Box::pin(async move {
        // Boxed for `clippy::large_stack_frames`: the three monomorphized
        // deploy futures would otherwise each reserve their full size in
        // this frame.
        {
            let config_root = derive_config_root(cfg, dir, reporter)
                .wrap_err("derive workspace root and package manager policy")?;
            let pipeline = DeployPipeline { args, cfg, config_root };
            match reporter {
                ReporterType::Default | ReporterType::AppendOnly => {
                    Box::pin(pipeline.run::<DefaultReporter>(dir)).await?;
                }
                ReporterType::Ndjson => {
                    Box::pin(pipeline.run::<NdjsonReporter>(dir)).await?;
                }
                ReporterType::Silent => {
                    Box::pin(pipeline.run::<SilentReporter>(dir)).await?;
                }
            }
        }
        Ok(())
    }))
}

pub(in super::super) fn dedupe<'a>(
    ctx: &RunCtx<'a>,
    args: DedupeArgs,
) -> miette::Result<CommandFuture<'a>> {
    let dir = ctx.locations.dir;
    let manifest_path = ctx.locations.manifest_path;
    let reporter = ctx.reporter();
    let config = ctx.loaders.config;
    Ok(Box::pin(async move {
        let cfg = config()?;
        args.apply_cli_config(cfg);
        let config_root = derive_config_root(cfg, dir, reporter)
            .wrap_err("derive workspace root and package manager policy")?;
        let recursive_sort = cfg.sort;
        let dedupe = DedupePipeline {
            args,
            cfg,
            config_root,
            prefix: dir.to_path_buf(),
            manifest_path: manifest_path.to_path_buf(),
            recursive_sort,
        };
        match reporter {
            ReporterType::Default | ReporterType::AppendOnly => {
                Box::pin(dedupe.run::<DefaultReporter>()).await?;
            }
            ReporterType::Ndjson => {
                Box::pin(dedupe.run::<NdjsonReporter>()).await?;
            }
            ReporterType::Silent => {
                Box::pin(dedupe.run::<SilentReporter>()).await?;
            }
        }
        Ok(())
    }))
}

pub(in super::super) fn prune<'a>(
    ctx: &RunCtx<'a>,
    args: PruneArgs,
) -> miette::Result<CommandFuture<'a>> {
    let dir = ctx.locations.dir;
    let manifest_path = ctx.locations.manifest_path;
    let reporter = ctx.reporter();
    let config = ctx.loaders.config;
    Ok(Box::pin(async move {
        let cfg = config()?;
        let config_root = derive_config_root(cfg, dir, reporter)
            .wrap_err("derive workspace root and package manager policy")?;
        let pipeline =
            PrunePipeline { args, cfg, config_root, manifest_path: manifest_path.to_path_buf() };
        match reporter {
            ReporterType::Default | ReporterType::AppendOnly => {
                Box::pin(pipeline.run::<DefaultReporter>()).await?;
            }
            ReporterType::Ndjson => {
                Box::pin(pipeline.run::<NdjsonReporter>()).await?;
            }
            ReporterType::Silent => {
                Box::pin(pipeline.run::<SilentReporter>()).await?;
            }
        }
        Ok(())
    }))
}

pub(in super::super) fn fetch<'a>(
    ctx: &RunCtx<'a>,
    args: FetchArgs,
) -> miette::Result<CommandFuture<'a>> {
    let ignore_pnpmfile = args.ignore_pnpmfile;
    let command_state = ctx.prepared_state_with(true, move |config| {
        config.ignore_pnpmfile |= ignore_pnpmfile;
    });
    Ok(match ctx.reporter() {
        ReporterType::Default | ReporterType::AppendOnly => {
            Box::pin(async move { args.run::<DefaultReporter>(command_state.await?).await })
        }
        ReporterType::Ndjson => {
            Box::pin(async move { args.run::<NdjsonReporter>(command_state.await?).await })
        }
        ReporterType::Silent => {
            Box::pin(async move { args.run::<SilentReporter>(command_state.await?).await })
        }
    })
}

pub(in super::super) fn import<'a>(
    ctx: &RunCtx<'a>,
    args: ImportArgs,
) -> miette::Result<CommandFuture<'a>> {
    let command_state = ctx.prepared_state(false);
    let reporter = ctx.reporter();
    Ok(Box::pin(async move {
        let command_state = command_state.await?;
        match reporter {
            ReporterType::Default | ReporterType::AppendOnly => {
                args.run::<DefaultReporter>(command_state).await
            }
            ReporterType::Ndjson => args.run::<NdjsonReporter>(command_state).await,
            ReporterType::Silent => args.run::<SilentReporter>(command_state).await,
        }
    }))
}

pub(in super::super) fn link<'a>(
    ctx: &RunCtx<'a>,
    args: LinkArgs,
) -> miette::Result<CommandFuture<'a>> {
    let config = (ctx.loaders.config)()?;
    let dir = ctx.locations.dir;
    let manifest_path = ctx.locations.manifest_path.to_path_buf();
    let reporter = ctx.reporter();
    Ok(Box::pin(async move {
        apply_update_config(config, dir, reporter).await?;
        match reporter {
            ReporterType::Default | ReporterType::AppendOnly => {
                args.run::<DefaultReporter>(config, manifest_path).await
            }
            ReporterType::Ndjson => args.run::<NdjsonReporter>(config, manifest_path).await,
            ReporterType::Silent => args.run::<SilentReporter>(config, manifest_path).await,
        }
    }))
}

pub(in super::super) fn unlink<'a>(
    ctx: &RunCtx<'a>,
    args: UnlinkArgs,
) -> miette::Result<CommandFuture<'a>> {
    let dir = ctx.locations.dir;
    let manifest_path = ctx.locations.manifest_path;
    let reporter = ctx.reporter();
    let config = ctx.loaders.config;
    Ok(Box::pin(async move {
        let cfg = config()?;
        let recursive_sort = cfg.sort;
        args.apply_cli_config(cfg);
        // Strip the matching `link:` overrides; stop early when there is
        // nothing to unlink.
        if !args.strip_link_overrides(cfg, manifest_path)? {
            return Ok(());
        }
        // Reinstall through the install-family pipeline, exactly as pnpm's
        // `unlink` delegates to its install handler, so `-r` / `--filter`
        // selection and per-project lockfiles apply. The reinstall forces a
        // fresh resolution so the removed `link:` overrides re-resolve from
        // the registry.
        let config_root = derive_config_root(cfg, dir, reporter)
            .wrap_err("derive workspace root and package manager policy")?;
        let pipeline = InstallPipeline {
            args: InstallArgs::for_reresolving_install(),
            cfg,
            config_root,
            prefix: dir.to_path_buf(),
            manifest_path: manifest_path.to_path_buf(),
            recursive_sort,
            require_lockfile: false,
            frozen_lockfile: false,
        };
        match reporter {
            ReporterType::Default | ReporterType::AppendOnly => {
                Box::pin(pipeline.run::<DefaultReporter>()).await?;
            }
            ReporterType::Ndjson => {
                Box::pin(pipeline.run::<NdjsonReporter>()).await?;
            }
            ReporterType::Silent => {
                Box::pin(pipeline.run::<SilentReporter>()).await?;
            }
        }
        Ok(())
    }))
}

pub(in super::super) fn rebuild<'a>(
    ctx: &RunCtx<'a>,
    mut args: RebuildArgs,
    command_name: &'static str,
) -> miette::Result<CommandFuture<'a>> {
    let dir = ctx.locations.dir;
    let manifest_path = ctx.locations.manifest_path;
    let reporter = ctx.reporter();
    let config = ctx.loaders.config;
    let cfg = config()?;
    if let Some(run_args) = script_override::resolve(ctx, cfg, command_name, args.packages.clone())?
    {
        return dispatch_script::run(ctx, run_args);
    }
    Ok(Box::pin(async move {
        apply_update_config(cfg, dir, reporter).await?;
        let recursive_sort = cfg.sort;
        let recursive_no_bail = !cfg.bail;
        args.pending = resolve_bool_override(args.pending, args.no_pending, cfg.pending);
        run_rebuild_args(
            args,
            cfg,
            dir,
            manifest_path,
            recursive_sort,
            recursive_no_bail,
            reporter,
        )
        .await?;
        Ok(())
    }))
}

pub(in super::super) fn runtime<'a>(
    ctx: &RunCtx<'a>,
    args: RuntimeArgs,
) -> miette::Result<CommandFuture<'a>> {
    if args.global {
        let config = (ctx.loaders.global_config)()?;
        let dir = ctx.locations.dir;
        return Ok(match ctx.reporter() {
            ReporterType::Default | ReporterType::AppendOnly => {
                Box::pin(args.run_global::<DefaultReporter>(config, dir))
            }
            ReporterType::Ndjson => Box::pin(args.run_global::<NdjsonReporter>(config, dir)),
            ReporterType::Silent => Box::pin(args.run_global::<SilentReporter>(config, dir)),
        });
    }
    let command_state = ctx.prepared_state(false);
    Ok(match ctx.reporter() {
        ReporterType::Default | ReporterType::AppendOnly => {
            Box::pin(async move { args.run::<DefaultReporter>(command_state.await?).await })
        }
        ReporterType::Ndjson => {
            Box::pin(async move { args.run::<NdjsonReporter>(command_state.await?).await })
        }
        ReporterType::Silent => {
            Box::pin(async move { args.run::<SilentReporter>(command_state.await?).await })
        }
    })
}

// `pnpm env use` installs a runtime globally, so it takes the same
// global-config load `runtime set -g` does; `pnpm env list` only queries a
// mirror and needs no install pipeline at all.
pub(in super::super) fn env<'a>(
    ctx: &RunCtx<'a>,
    args: EnvArgs,
) -> miette::Result<CommandFuture<'a>> {
    let config = (ctx.loaders.global_config)()?;
    let dir = ctx.locations.dir;
    // The reporter is chosen before the subcommand is classified because
    // classifying `env use` already emits its deprecation warning.
    match ctx.reporter() {
        ReporterType::Default | ReporterType::AppendOnly => {
            env_with_reporter::<DefaultReporter>(args, config, dir)
        }
        ReporterType::Ndjson => env_with_reporter::<NdjsonReporter>(args, config, dir),
        ReporterType::Silent => env_with_reporter::<SilentReporter>(args, config, dir),
    }
}

fn env_with_reporter<'a, Reporter: pnpm_reporter::Reporter + 'static>(
    args: EnvArgs,
    config: &'static Config,
    dir: &'a Path,
) -> miette::Result<CommandFuture<'a>> {
    Ok(match args.subcommand::<Reporter>(config)? {
        EnvSubcommand::Use { package_name } => {
            Box::pin(EnvArgs::run_use::<Reporter>(package_name, config, dir))
        }
        EnvSubcommand::List { version_spec } => Box::pin(async move {
            println!("{}", EnvArgs::run_list(version_spec, config).await?);
            Ok(())
        }),
    })
}

pub(in super::super) fn approve_builds<'a>(
    ctx: &RunCtx<'a>,
    args: ApproveBuildsArgs,
) -> miette::Result<CommandFuture<'a>> {
    if args.global {
        let config = (ctx.loaders.global_config)()?;
        return Ok(approve_global_builds(config, args, ctx.reporter()));
    }
    let config = ctx.prepared_config();
    let dir = ctx.locations.dir;
    let manifest_path = ctx.locations.manifest_path;
    macro_rules! run_approve_builds {
        ($reporter:ty) => {
            Box::pin(async move {
                let config = config.await?;
                let Some((rebuild_state, build_packages)) =
                    args.prepare::<$reporter>(dir, config, config, manifest_path)?
                else {
                    return Ok(());
                };
                let selected =
                    rebuild::RebuildSelection { names: Some(build_packages), projects: Vec::new() };
                rebuild::run_rebuild::<$reporter>(&rebuild_state, selected, None).await
            })
        };
    }
    Ok(match ctx.reporter() {
        ReporterType::Default | ReporterType::AppendOnly => run_approve_builds!(DefaultReporter),
        ReporterType::Ndjson => run_approve_builds!(NdjsonReporter),
        ReporterType::Silent => run_approve_builds!(SilentReporter),
    })
}

async fn run_rebuild_args(
    args: RebuildArgs,
    cfg: &'static Config,
    dir: &Path,
    manifest_path: &Path,
    recursive_sort: bool,
    recursive_no_bail: bool,
    reporter: ReporterType,
) -> miette::Result<()> {
    match reporter {
        ReporterType::Default | ReporterType::AppendOnly => {
            Box::pin(args.run_from_cli::<DefaultReporter>(
                cfg,
                dir.to_path_buf(),
                manifest_path.to_path_buf(),
                recursive_sort,
                recursive_no_bail,
            ))
            .await?;
        }
        ReporterType::Ndjson => {
            Box::pin(args.run_from_cli::<NdjsonReporter>(
                cfg,
                dir.to_path_buf(),
                manifest_path.to_path_buf(),
                recursive_sort,
                recursive_no_bail,
            ))
            .await?;
        }
        ReporterType::Silent => {
            Box::pin(args.run_from_cli::<SilentReporter>(
                cfg,
                dir.to_path_buf(),
                manifest_path.to_path_buf(),
                recursive_sort,
                recursive_no_bail,
            ))
            .await?;
        }
    }
    Ok(())
}

fn approve_global_builds<'a>(
    config: &'static Config,
    args: ApproveBuildsArgs,
    reporter: ReporterType,
) -> CommandFuture<'a> {
    match reporter {
        ReporterType::Default | ReporterType::AppendOnly => {
            Box::pin(global::approve_global_builds::<DefaultReporter>(config, args))
        }
        ReporterType::Ndjson => {
            Box::pin(global::approve_global_builds::<NdjsonReporter>(config, args))
        }
        ReporterType::Silent => {
            Box::pin(global::approve_global_builds::<SilentReporter>(config, args))
        }
    }
}
