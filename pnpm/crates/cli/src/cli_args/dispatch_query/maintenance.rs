use super::{
    BinArgs, BugsArgs, CacheCommand, CatFileArgs, CatIndexArgs, CleanArgs, CommandFuture, Config,
    ConfigArgs, ConfigGetAliasArgs, ConfigSetAliasArgs, ConfigSubcommand, DefaultReporter,
    DocsArgs, DoctorArgs, DoctorOutcome, FindHashArgs, IgnoredBuildsArgs, NdjsonReporter,
    NotImplementedError, PrefixArgs, RepoArgs, ReporterType, RootArgs, RunCtx, SelfUpdateArgs,
    SetupArgs, ShimArgs, SilentReporter, StoreCommand, WithArgs,
};

// `doctor` reports on the installation and its environment, so it needs config
// resolved but no lockfile or install pipeline. It returns the rendered report
// rather than printing it, mirroring pnpm's handler → CLI print split, and the
// exit lives here because a failing check must fail the command — that is what
// lets the release pipeline gate a promotion on it.
pub(in super::super) fn doctor<'a>(
    ctx: &RunCtx<'a>,
    args: DoctorArgs,
) -> miette::Result<CommandFuture<'a>> {
    let cfg: &Config = (ctx.config)()?;
    Ok(Box::pin(async move {
        let result = args.run(cfg).await?;
        println!("{}", result.output);
        if result.outcome == DoctorOutcome::Unhealthy {
            #[expect(
                clippy::exit,
                reason = "`doctor` exits non-zero when a check fails, mirroring pnpm"
            )]
            std::process::exit(1);
        }
        Ok(())
    }))
}

pub(in super::super) fn bin<'a>(
    ctx: &RunCtx<'a>,
    args: BinArgs,
) -> miette::Result<CommandFuture<'a>> {
    args.run(ctx.dir, (ctx.config)()?)?;
    Ok(Box::pin(std::future::ready(Ok(()))))
}

pub(in super::super) fn clean<'a>(
    ctx: &RunCtx<'a>,
    args: CleanArgs,
    command_name: &'a str,
) -> miette::Result<CommandFuture<'a>> {
    args.run(ctx, command_name)?;
    Ok(Box::pin(std::future::ready(Ok(()))))
}

pub(in super::super) fn root<'a>(
    ctx: &RunCtx<'a>,
    args: RootArgs,
) -> miette::Result<CommandFuture<'a>> {
    args.run(ctx.dir, (ctx.config)()?)?;
    Ok(Box::pin(std::future::ready(Ok(()))))
}

pub(in super::super) fn prefix<'a>(
    ctx: &RunCtx<'a>,
    args: PrefixArgs,
) -> miette::Result<CommandFuture<'a>> {
    args.run(ctx.dir, (ctx.config)()?)?;
    Ok(Box::pin(std::future::ready(Ok(()))))
}

pub(in super::super) fn shim<'a>(
    ctx: &RunCtx<'a>,
    args: ShimArgs,
) -> miette::Result<CommandFuture<'a>> {
    // Writes the global bin directory and the global `config.yaml`, so it
    // reads the configuration anchored at the pnpm home — a project the
    // command happens to run in does not get to steer either.
    let config = (ctx.global_config)()?;
    Ok(Box::pin(async move {
        print!("{}", args.run(config).await?);
        Ok(())
    }))
}

pub(in super::super) fn config<'a>(
    ctx: &RunCtx<'a>,
    args: ConfigArgs,
) -> miette::Result<CommandFuture<'a>> {
    args.run((ctx.config)()?, ctx.dir)?;
    Ok(Box::pin(std::future::ready(Ok(()))))
}

// `pnpm get` / `pnpm set` are the top-level spellings of the two most-used
// `pnpm config` subcommands, and run the same code so the two spellings
// cannot drift.
pub(in super::super) fn config_get<'a>(
    ctx: &RunCtx<'a>,
    args: ConfigGetAliasArgs,
) -> miette::Result<CommandFuture<'a>> {
    config(ctx, ConfigArgs { flags: args.flags, command: ConfigSubcommand::Get(args.args) })
}

pub(in super::super) fn config_set<'a>(
    ctx: &RunCtx<'a>,
    args: ConfigSetAliasArgs,
) -> miette::Result<CommandFuture<'a>> {
    config(ctx, ConfigArgs { flags: args.flags, command: ConfigSubcommand::Set(args.args) })
}

pub(in super::super) fn not_implemented<'a>(
    command: &'static str,
) -> miette::Result<CommandFuture<'a>> {
    Err(NotImplementedError { command }.into())
}

pub(in super::super) fn repo<'a>(
    ctx: &RunCtx<'a>,
    args: RepoArgs,
) -> miette::Result<CommandFuture<'a>> {
    let cfg = (ctx.config)()?;
    let dir = ctx.dir;
    Ok(match ctx.reporter {
        ReporterType::Default | ReporterType::AppendOnly => Box::pin(async move {
            args.run::<pnpm_network_web_auth::Host, DefaultReporter>(cfg, dir).await
        }),
        ReporterType::Ndjson => Box::pin(async move {
            args.run::<pnpm_network_web_auth::Host, NdjsonReporter>(cfg, dir).await
        }),
        ReporterType::Silent => Box::pin(async move {
            args.run::<pnpm_network_web_auth::Host, SilentReporter>(cfg, dir).await
        }),
    })
}

pub(in super::super) fn docs<'a>(
    ctx: &RunCtx<'a>,
    args: DocsArgs,
) -> miette::Result<CommandFuture<'a>> {
    let cfg = (ctx.config)()?;
    Ok(Box::pin(async move { args.run::<pnpm_network_web_auth::Host>(cfg).await }))
}

pub(in super::super) fn with<'a>(
    ctx: &RunCtx<'a>,
    args: WithArgs,
) -> miette::Result<CommandFuture<'a>> {
    let config = (ctx.config)()?;
    macro_rules! run_with {
        ($reporter:ty) => {
            Box::pin(args.run::<$reporter>(config))
        };
    }
    Ok(match ctx.reporter {
        ReporterType::Default | ReporterType::AppendOnly => run_with!(DefaultReporter),
        ReporterType::Ndjson => run_with!(NdjsonReporter),
        ReporterType::Silent => run_with!(SilentReporter),
    })
}

pub(in super::super) fn self_update<'a>(
    ctx: &RunCtx<'a>,
    args: SelfUpdateArgs,
) -> miette::Result<CommandFuture<'a>> {
    // Refuse corepack before loading project config, so a broken `.npmrc`
    // / workspace config can't mask the corepack refusal.
    super::super::self_update::reject_if_corepack()?;
    let config = (ctx.config_self_update)()?;
    let dir = ctx.dir;
    macro_rules! run_self_update {
        ($reporter:ty) => {
            Box::pin(args.run::<$reporter>(config, dir))
        };
    }
    Ok(match ctx.reporter {
        ReporterType::Default | ReporterType::AppendOnly => run_self_update!(DefaultReporter),
        ReporterType::Ndjson => run_self_update!(NdjsonReporter),
        ReporterType::Silent => run_self_update!(SilentReporter),
    })
}

// `setup` makes pnpm available globally: it installs the CLI into the
// global packages dir, writes the alias scripts, and persists `PNPM_HOME` /
// PATH into the user's shell rc file (POSIX) or registry (Windows). It needs
// a reporter for the "Installing pnpm CLI globally" log but no project
// config or lockfile, so it dispatches off `ctx.dir` like the other
// reporter-typed commands.
pub(in super::super) fn setup<'a>(
    ctx: &RunCtx<'a>,
    args: SetupArgs,
) -> miette::Result<CommandFuture<'a>> {
    let dir = ctx.dir;
    macro_rules! run_setup {
        ($reporter:ty) => {
            Box::pin(args.run::<$reporter>(dir))
        };
    }
    Ok(match ctx.reporter {
        ReporterType::Default | ReporterType::AppendOnly => run_setup!(DefaultReporter),
        ReporterType::Ndjson => run_setup!(NdjsonReporter),
        ReporterType::Silent => run_setup!(SilentReporter),
    })
}

pub(in super::super) fn store<'a>(
    ctx: &RunCtx<'a>,
    command: StoreCommand,
) -> miette::Result<CommandFuture<'a>> {
    let config: &Config = (ctx.config)()?;
    let dir = ctx.dir;
    Ok(match ctx.reporter {
        ReporterType::Default | ReporterType::AppendOnly => {
            Box::pin(command.run::<DefaultReporter>(config, dir))
        }
        ReporterType::Ndjson => Box::pin(command.run::<NdjsonReporter>(config, dir)),
        ReporterType::Silent => Box::pin(command.run::<SilentReporter>(config, dir)),
    })
}

pub(in super::super) fn cache<'a>(
    ctx: &RunCtx<'a>,
    command: CacheCommand,
) -> miette::Result<CommandFuture<'a>> {
    command.run((ctx.config)()?)?;
    Ok(Box::pin(std::future::ready(Ok(()))))
}

pub(in super::super) fn cat_file<'a>(
    ctx: &RunCtx<'a>,
    args: CatFileArgs,
) -> miette::Result<CommandFuture<'a>> {
    args.run(|| (ctx.config)().map(|m| &*m))?;
    Ok(Box::pin(std::future::ready(Ok(()))))
}

pub(in super::super) fn cat_index<'a>(
    ctx: &RunCtx<'a>,
    args: CatIndexArgs,
) -> miette::Result<CommandFuture<'a>> {
    let dir = ctx.dir;
    let config = ctx.config;
    Ok(Box::pin(async move {
        args.run(dir, || config().map(|m| &*m)).await?;
        Ok(())
    }))
}

pub(in super::super) fn ignored_builds<'a>(
    ctx: &RunCtx<'a>,
    _args: IgnoredBuildsArgs,
) -> miette::Result<CommandFuture<'a>> {
    let output = super::super::ignored_builds::render_ignored_builds((ctx.config)()?)?;
    print!("{output}");
    Ok(Box::pin(std::future::ready(Ok(()))))
}

pub(in super::super) fn bugs<'a>(
    ctx: &RunCtx<'a>,
    args: BugsArgs,
) -> miette::Result<CommandFuture<'a>> {
    let cfg: &Config = (ctx.config)()?;
    let dir = ctx.dir;
    Ok(Box::pin(async move { args.run::<pnpm_network_web_auth::Host>(cfg, dir).await }))
}

pub(in super::super) fn find_hash<'a>(
    ctx: &RunCtx<'a>,
    args: FindHashArgs,
) -> miette::Result<CommandFuture<'a>> {
    args.run(|| (ctx.config)().map(|m| &*m))?;
    Ok(Box::pin(std::future::ready(Ok(()))))
}
