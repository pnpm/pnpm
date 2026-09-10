use super::{
    super::rebuild, ApproveBuildsArgs, CommandFuture, Config, Context, DedupeArgs, DedupePipeline,
    DefaultReporter, DeployArgs, DeployPipeline, EnvArgs, EnvSubcommand, FetchArgs, ImportArgs,
    InstallArgs, InstallPipeline, LinkArgs, NdjsonReporter, Path, PruneArgs, PrunePipeline,
    RebuildArgs, ReporterType, RunCtx, RuntimeArgs, SilentReporter, State, UnlinkArgs,
    apply_install_cli_config, apply_update_config, derive_config_root, global,
    resolve_bool_override,
};

pub(in super::super) fn deploy<'a>(
    ctx: &RunCtx<'a>,
    args: DeployArgs,
) -> miette::Result<CommandFuture<'a>> {
    let dir = ctx.dir;
    let reporter = ctx.reporter;
    let config = ctx.config;
    Ok(Box::pin(async move {
        // Boxed for `clippy::large_stack_frames`: the three monomorphized
        // deploy futures would otherwise each reserve their full size in
        // this frame.
        {
            let cfg = config()?;
            apply_install_cli_config(cfg, &args.install_args);
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
    let dir = ctx.dir;
    let manifest_path = ctx.manifest_path;
    let reporter = ctx.reporter;
    let config = ctx.config;
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
            ReporterType::Ndjson => Box::pin(dedupe.run::<NdjsonReporter>()).await?,
            ReporterType::Silent => Box::pin(dedupe.run::<SilentReporter>()).await?,
        }
        Ok(())
    }))
}

pub(in super::super) fn prune<'a>(
    ctx: &RunCtx<'a>,
    args: PruneArgs,
) -> miette::Result<CommandFuture<'a>> {
    let dir = ctx.dir;
    let manifest_path = ctx.manifest_path;
    let reporter = ctx.reporter;
    let config = ctx.config;
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
    Ok(match ctx.reporter {
        ReporterType::Default | ReporterType::AppendOnly => {
            Box::pin(args.run::<DefaultReporter>((ctx.state)(true)?))
        }
        ReporterType::Ndjson => Box::pin(args.run::<NdjsonReporter>((ctx.state)(true)?)),
        ReporterType::Silent => Box::pin(args.run::<SilentReporter>((ctx.state)(true)?)),
    })
}

pub(in super::super) fn import<'a>(
    ctx: &RunCtx<'a>,
    args: ImportArgs,
) -> miette::Result<CommandFuture<'a>> {
    let config = (ctx.config)()?;
    let dir = ctx.dir;
    let manifest_path = ctx.manifest_path.to_path_buf();
    let reporter = ctx.reporter;
    Ok(Box::pin(async move {
        apply_update_config(config, dir, reporter).await?;
        let command_state =
            State::init(manifest_path, config, false).wrap_err("initialize the state")?;
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
    let config = (ctx.config)()?;
    let dir = ctx.dir;
    let manifest_path = ctx.manifest_path.to_path_buf();
    let reporter = ctx.reporter;
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
    let dir = ctx.dir;
    let manifest_path = ctx.manifest_path;
    let reporter = ctx.reporter;
    let config = ctx.config;
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
            ReporterType::Ndjson => Box::pin(pipeline.run::<NdjsonReporter>()).await?,
            ReporterType::Silent => Box::pin(pipeline.run::<SilentReporter>()).await?,
        }
        Ok(())
    }))
}

pub(in super::super) fn rebuild<'a>(
    ctx: &RunCtx<'a>,
    mut args: RebuildArgs,
) -> miette::Result<CommandFuture<'a>> {
    let dir = ctx.dir;
    let manifest_path = ctx.manifest_path;
    let reporter = ctx.reporter;
    let config = ctx.config;
    Ok(Box::pin(async move {
        let cfg = config()?;
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
        let config = (ctx.global_config)()?;
        let dir = ctx.dir;
        return Ok(match ctx.reporter {
            ReporterType::Default | ReporterType::AppendOnly => {
                Box::pin(args.run_global::<DefaultReporter>(config, dir))
            }
            ReporterType::Ndjson => Box::pin(args.run_global::<NdjsonReporter>(config, dir)),
            ReporterType::Silent => Box::pin(args.run_global::<SilentReporter>(config, dir)),
        });
    }
    let command_state = (ctx.state)(false)?;
    Ok(match ctx.reporter {
        ReporterType::Default | ReporterType::AppendOnly => {
            Box::pin(args.run::<DefaultReporter>(command_state))
        }
        ReporterType::Ndjson => Box::pin(args.run::<NdjsonReporter>(command_state)),
        ReporterType::Silent => Box::pin(args.run::<SilentReporter>(command_state)),
    })
}

// `pnpm env use` installs a runtime globally, so it takes the same
// global-config load `runtime set -g` does; `pnpm env list` only queries a
// mirror and needs no install pipeline at all.
pub(in super::super) fn env<'a>(
    ctx: &RunCtx<'a>,
    args: EnvArgs,
) -> miette::Result<CommandFuture<'a>> {
    let config = (ctx.global_config)()?;
    let dir = ctx.dir;
    // The reporter is chosen before the subcommand is classified because
    // classifying `env use` already emits its deprecation warning.
    match ctx.reporter {
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
        let config = (ctx.global_config)()?;
        return Ok(match ctx.reporter {
            ReporterType::Default | ReporterType::AppendOnly => {
                Box::pin(global::approve_global_builds::<DefaultReporter>(config, args))
            }
            ReporterType::Ndjson => {
                Box::pin(global::approve_global_builds::<NdjsonReporter>(config, args))
            }
            ReporterType::Silent => {
                Box::pin(global::approve_global_builds::<SilentReporter>(config, args))
            }
        });
    }
    // The settings/prompt work is synchronous; only the rebuild is async, so
    // the non-`Send` `config` / `state` closures stay out of the awaited
    // future.
    let prepared = match ctx.reporter {
        ReporterType::Default | ReporterType::AppendOnly => {
            args.prepare::<DefaultReporter>(ctx.dir, ctx.config, ctx.state)
        }
        ReporterType::Ndjson => args.prepare::<NdjsonReporter>(ctx.dir, ctx.config, ctx.state),
        ReporterType::Silent => args.prepare::<SilentReporter>(ctx.dir, ctx.config, ctx.state),
    };
    let Some((rebuild_state, build_packages)) = prepared? else {
        return Ok(Box::pin(std::future::ready(Ok(()))));
    };
    let selected = rebuild::RebuildSelection { names: Some(build_packages), projects: Vec::new() };
    Ok(match ctx.reporter {
        ReporterType::Default | ReporterType::AppendOnly => Box::pin(async move {
            rebuild::run_rebuild::<DefaultReporter>(&rebuild_state, selected, None).await
        }),
        ReporterType::Ndjson => Box::pin(async move {
            rebuild::run_rebuild::<NdjsonReporter>(&rebuild_state, selected, None).await
        }),
        ReporterType::Silent => Box::pin(async move {
            rebuild::run_rebuild::<SilentReporter>(&rebuild_state, selected, None).await
        }),
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
