use super::{
    super::{dispatch_script, rebuild, script_override},
    ApproveBuildsArgs, CommandFuture, Context, DedupeArgs, DedupePipeline, DeployArgs,
    DeployPipeline, EnvArgs, EnvSubcommand, FetchArgs, ImportArgs, InstallArgs, InstallPipeline,
    LinkArgs, PruneArgs, PrunePipeline, RebuildArgs, RunCtx, RuntimeArgs, UnlinkArgs,
    apply_install_cli_config, apply_update_config, derive_config_root, global,
    installed_project_config, resolve_bool_override, warn_about_config_root,
};
use crate::cli_args::reporter::{CliReporter, selected_reporter};

pub(in super::super) fn deploy<'a>(
    ctx: &RunCtx<'a>,
    args: DeployArgs,
) -> miette::Result<CommandFuture<'a>> {
    let dir = ctx.locations.dir;
    let cfg = (ctx.loaders.config)()?;
    let reporter = selected_reporter();
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
        let config_root = derive_config_root(&mut *cfg, dir, reporter)
            .wrap_err("derive workspace root and package manager policy")?;
        let pipeline = DeployPipeline { args, cfg, config_root };
        Box::pin(pipeline.run::<CliReporter>(dir)).await?;
        Ok(())
    }))
}

pub(in super::super) fn dedupe<'a>(
    ctx: &RunCtx<'a>,
    args: DedupeArgs,
) -> miette::Result<CommandFuture<'a>> {
    let dir = ctx.locations.dir;
    let manifest_path = ctx.locations.manifest_path;
    let cfg = (ctx.loaders.config)()?;
    let reporter = selected_reporter();
    Ok(Box::pin(async move {
        args.apply_cli_config(cfg);
        let config_root = derive_config_root(&mut *cfg, dir, reporter)
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
        Box::pin(dedupe.run::<CliReporter>()).await?;
        Ok(())
    }))
}

pub(in super::super) fn prune<'a>(
    ctx: &RunCtx<'a>,
    args: PruneArgs,
) -> miette::Result<CommandFuture<'a>> {
    let dir = ctx.locations.dir;
    let manifest_path = ctx.locations.manifest_path;
    let cfg = (ctx.loaders.config)()?;
    let reporter = selected_reporter();
    Ok(Box::pin(async move {
        let config_root = derive_config_root(&mut *cfg, dir, reporter)
            .wrap_err("derive workspace root and package manager policy")?;
        let pipeline =
            PrunePipeline { args, cfg, config_root, manifest_path: manifest_path.to_path_buf() };
        Box::pin(pipeline.run::<CliReporter>()).await?;
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
    Ok(Box::pin(async move {
        let command_state = command_state.await?;
        args.run::<CliReporter>(command_state).await
    }))
}

pub(in super::super) fn import<'a>(
    ctx: &RunCtx<'a>,
    args: ImportArgs,
) -> miette::Result<CommandFuture<'a>> {
    let dir = ctx.locations.dir;
    let command_state = ctx.prepared_state(false);
    Ok(Box::pin(async move {
        let command_state = command_state.await?;
        let reporter = selected_reporter();
        warn_about_config_root(command_state.config, dir, reporter)?;
        args.run::<CliReporter>(command_state).await
    }))
}

pub(in super::super) fn link<'a>(
    ctx: &RunCtx<'a>,
    args: LinkArgs,
) -> miette::Result<CommandFuture<'a>> {
    let config = (ctx.loaders.config)()?;
    let dir = ctx.locations.dir;
    let manifest_path = ctx.locations.manifest_path.to_path_buf();
    Ok(Box::pin(async move {
        apply_update_config(config, dir).await?;
        args.run::<CliReporter>(config, manifest_path).await
    }))
}

pub(in super::super) fn unlink<'a>(
    ctx: &RunCtx<'a>,
    args: UnlinkArgs,
) -> miette::Result<CommandFuture<'a>> {
    let dir = ctx.locations.dir;
    let manifest_path = ctx.locations.manifest_path;
    let cfg = (ctx.loaders.config)()?;
    let reporter = selected_reporter();
    Ok(Box::pin(async move {
        let recursive_sort = cfg.sort;
        args.apply_cli_config(cfg);
        if !args.remove_links(cfg, dir, manifest_path, recursive_sort)? {
            return Ok(());
        }
        // Reinstall through the install-family pipeline, exactly as pnpm's
        // `unlink` delegates to its install handler, so `-r` / `--filter`
        // selection and per-project lockfiles apply. The reinstall forces a
        // fresh resolution so the removed `link:` overrides re-resolve from
        // the registry.
        let config_root = derive_config_root(&mut *cfg, dir, reporter)
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
        Box::pin(pipeline.run::<CliReporter>()).await?;
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
    let cfg = (ctx.loaders.config)()?;
    if let Some(run_args) = script_override::resolve(ctx, cfg, command_name, args.packages.clone())?
    {
        return dispatch_script::run(ctx, run_args);
    }
    Ok(Box::pin(async move {
        apply_update_config(cfg, dir).await?;
        let recursive_sort = cfg.sort;
        let recursive_no_bail = !cfg.bail;
        args.pending = resolve_bool_override(args.pending, args.no_pending, cfg.pending);
        Box::pin(args.run_from_cli::<CliReporter>(
            cfg,
            dir.to_path_buf(),
            manifest_path.to_path_buf(),
            recursive_sort,
            recursive_no_bail,
        ))
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
        return Ok(Box::pin(args.run_global::<CliReporter>(config, dir)));
    }
    let command_state = ctx.prepared_state(false);
    Ok(Box::pin(async move {
        let command_state = command_state.await?;
        args.run::<CliReporter>(command_state).await
    }))
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
    Ok(match args.subcommand::<CliReporter>(config)? {
        EnvSubcommand::Use { package_name } => {
            Box::pin(EnvArgs::run_use::<CliReporter>(package_name, config, dir))
        }
        EnvSubcommand::Remove { versions } => {
            Box::pin(EnvArgs::run_remove::<CliReporter>(versions, config, dir))
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
        return Ok(Box::pin(global::approve_global_builds::<CliReporter>(config, args)));
    }
    let config = ctx.prepared_config();
    let dir = ctx.locations.dir;
    let manifest_path = ctx.locations.manifest_path;
    Ok(Box::pin(async move {
        let config = installed_project_config(config.await?, manifest_path);
        let Some((rebuild_state, build_packages)) =
            args.prepare::<CliReporter>(dir, config, config, manifest_path)?
        else {
            return Ok(());
        };
        let selected =
            rebuild::RebuildSelection { names: Some(build_packages), projects: Vec::new() };
        Box::pin(rebuild::run_rebuild::<CliReporter>(&rebuild_state, selected, None)).await
    }))
}
