use super::{
    AccessArgs, CommandFuture, Config, DefaultReporter, DeprecateArgs, DistTagArgs, LoginArgs,
    LogoutArgs, NdjsonReporter, OwnerArgs, PingArgs, ReporterType, RunCtx, SearchArgs,
    SilentReporter, StarArgs, StarsArgs, TeamArgs, UndeprecateArgs, UnpublishArgs, UnstarArgs,
    ViewArgs,
};

// `whoami` is a read-only registry query: it resolves the default registry's
// auth header from config and GETs `-/whoami`, with no lockfile or install
// pipeline. It needs an async future for the request but no reporter-typed
// fan-out, so it dispatches off `config()` like the other read-only commands.
pub(in super::super) fn whoami<'a>(ctx: &RunCtx<'a>) -> miette::Result<CommandFuture<'a>> {
    let cfg: &Config = (ctx.config)()?;
    Ok(Box::pin(async move {
        let username = super::super::whoami::whoami(cfg).await?;
        println!("{}", super::super::sanitize::sanitize(&username));
        Ok(())
    }))
}

pub(in super::super) fn star<'a>(
    ctx: &RunCtx<'a>,
    args: StarArgs,
) -> miette::Result<CommandFuture<'a>> {
    let cfg: &Config = (ctx.config)()?;
    Ok(Box::pin(async move { args.run(cfg).await }))
}

pub(in super::super) fn unstar<'a>(
    ctx: &RunCtx<'a>,
    args: UnstarArgs,
) -> miette::Result<CommandFuture<'a>> {
    let cfg: &Config = (ctx.config)()?;
    Ok(Box::pin(async move { args.run(cfg).await }))
}

pub(in super::super) fn stars<'a>(
    ctx: &RunCtx<'a>,
    args: StarsArgs,
) -> miette::Result<CommandFuture<'a>> {
    let cfg: &Config = (ctx.config)()?;
    Ok(Box::pin(async move {
        if let Some(output) = args.run(cfg).await?
            && !output.is_empty()
        {
            println!("{output}");
        }
        Ok(())
    }))
}

pub(in super::super) fn access<'a>(
    ctx: &RunCtx<'a>,
    args: AccessArgs,
) -> miette::Result<CommandFuture<'a>> {
    let cfg: &Config = (ctx.config)()?;
    Ok(Box::pin(async move {
        if let Some(output) = args.run(cfg).await? {
            let output = super::super::sanitize::sanitize(&output);
            if output.is_empty() {
                return Ok(());
            }
            println!("{output}");
        }
        Ok(())
    }))
}

pub(in super::super) fn dist_tag<'a>(
    ctx: &RunCtx<'a>,
    args: DistTagArgs,
) -> miette::Result<CommandFuture<'a>> {
    let cfg: &Config = (ctx.config)()?;
    Ok(Box::pin(async move {
        if let Some(output) = args.run(cfg).await? {
            let output = super::super::sanitize::sanitize(&output);
            if output.is_empty() {
                return Ok(());
            }
            println!("{output}");
        }
        Ok(())
    }))
}

pub(in super::super) fn deprecate<'a>(
    ctx: &RunCtx<'a>,
    args: DeprecateArgs,
) -> miette::Result<CommandFuture<'a>> {
    let cfg: &Config = (ctx.config)()?;
    Ok(Box::pin(async move {
        if let Some(output) = args.run(cfg).await? {
            let output = super::super::sanitize::sanitize(&output);
            if output.is_empty() {
                return Ok(());
            }
            println!("{output}");
        }
        Ok(())
    }))
}

pub(in super::super) fn undeprecate<'a>(
    ctx: &RunCtx<'a>,
    args: UndeprecateArgs,
) -> miette::Result<CommandFuture<'a>> {
    let cfg: &Config = (ctx.config)()?;
    Ok(Box::pin(async move {
        if let Some(output) = args.run(cfg).await? {
            let output = super::super::sanitize::sanitize(&output);
            if output.is_empty() {
                return Ok(());
            }
            println!("{output}");
        }
        Ok(())
    }))
}

pub(in super::super) fn unpublish<'a>(
    ctx: &RunCtx<'a>,
    args: UnpublishArgs,
) -> miette::Result<CommandFuture<'a>> {
    let cfg: &Config = (ctx.config)()?;
    async fn print_output<Reporter: pnpm_reporter::Reporter>(
        args: UnpublishArgs,
        cfg: &Config,
    ) -> miette::Result<()> {
        if let Some(output) = args.run::<Reporter>(cfg).await? {
            let output = super::super::sanitize::sanitize(&output);
            if !output.is_empty() {
                println!("{output}");
            }
        }
        Ok(())
    }
    Ok(match ctx.reporter {
        ReporterType::Default | ReporterType::AppendOnly => {
            Box::pin(print_output::<DefaultReporter>(args, cfg))
        }
        ReporterType::Ndjson => Box::pin(print_output::<NdjsonReporter>(args, cfg)),
        ReporterType::Silent => Box::pin(print_output::<SilentReporter>(args, cfg)),
    })
}

pub(in super::super) fn team<'a>(
    ctx: &RunCtx<'a>,
    args: TeamArgs,
) -> miette::Result<CommandFuture<'a>> {
    let cfg: &Config = (ctx.config)()?;
    Ok(Box::pin(async move {
        if let Some(output) = args.run(cfg).await? {
            let output = super::super::sanitize::sanitize(&output);
            if output.is_empty() {
                return Ok(());
            }
            println!("{output}");
        }
        Ok(())
    }))
}

pub(in super::super) fn owner<'a>(
    ctx: &RunCtx<'a>,
    args: OwnerArgs,
) -> miette::Result<CommandFuture<'a>> {
    let cfg: &Config = (ctx.config)()?;
    Ok(Box::pin(async move {
        if let Some(output) = args.run(cfg).await? {
            let output = super::super::sanitize::sanitize(&output);
            if output.is_empty() {
                return Ok(());
            }
            println!("{output}");
        }
        Ok(())
    }))
}

// `ping` is a read-only connectivity check: it resolves the registry (and any
// auth header) from config and GETs `-/ping`, with no lockfile or install
// pipeline, so it dispatches off `config()` like the other read-only registry
// commands.
pub(in super::super) fn ping<'a>(
    ctx: &RunCtx<'a>,
    args: PingArgs,
) -> miette::Result<CommandFuture<'a>> {
    let cfg: &Config = (ctx.config)()?;
    Ok(Box::pin(async move {
        let report = args.run(cfg).await?;
        println!("{report}");
        Ok(())
    }))
}

// `view` is a read-only registry query: it resolves the package metadata
// (and, when the package name is omitted, the nearest manifest's name from
// `ctx.dir`), then prints the requested fields, a JSON dump, or the formatted
// summary its handler returns. No lockfile or install pipeline, so it
// dispatches off `config()` like the other read-only registry commands.
pub(in super::super) fn view<'a>(
    ctx: &RunCtx<'a>,
    args: ViewArgs,
) -> miette::Result<CommandFuture<'a>> {
    let cfg: &Config = (ctx.config)()?;
    let dir = ctx.dir;
    Ok(Box::pin(async move {
        let output = args.run(cfg, dir).await?;
        // A single-field selection of an absent field renders as an empty
        // string; skip the print so it emits no output. A multi-field `--json`
        // selection of absent fields renders as `{}` and is printed. Both
        // match `pnpm view`, which prints whatever truthy string the handler
        // returns.
        if !output.is_empty() {
            println!("{output}");
        }
        Ok(())
    }))
}

// `login` (a.k.a. `adduser`) authenticates with the registry and writes the
// token to `auth.ini`. Like `logout` it needs config (registry, config dir,
// network settings) but no lockfile or install pipeline. Its `globalInfo`
// messages (the auth URL / QR code, the "Logged in as ..." line) route through
// the reporter, so the reporter type is threaded through `run`.
pub(in super::super) fn login<'a>(
    ctx: &RunCtx<'a>,
    args: LoginArgs,
) -> miette::Result<CommandFuture<'a>> {
    let config: &Config = (ctx.config)()?;
    macro_rules! run_login {
        ($reporter:ty) => {
            Box::pin(async move { args.run::<$reporter>(config).await })
        };
    }
    Ok(match ctx.reporter {
        ReporterType::Default | ReporterType::AppendOnly => run_login!(DefaultReporter),
        ReporterType::Ndjson => run_login!(NdjsonReporter),
        ReporterType::Silent => run_login!(SilentReporter),
    })
}

// `logout` revokes the registry auth token and removes it from `auth.ini`. It
// needs config (registry, auth tokens, config dir, network settings) and the
// canonicalized `--dir` as the reporter `prefix`, but no lockfile or install
// pipeline. The reporter type only routes the `globalInfo` / `globalWarn`
// channels, so it's threaded through `run` like the other registry commands.
pub(in super::super) fn logout<'a>(
    ctx: &RunCtx<'a>,
    args: LogoutArgs,
) -> miette::Result<CommandFuture<'a>> {
    let config: &Config = (ctx.config)()?;
    let prefix = ctx.dir.to_string_lossy().into_owned();
    macro_rules! run_logout {
        ($reporter:ty) => {
            Box::pin(async move { args.run::<$reporter>(config, &prefix).await })
        };
    }
    Ok(match ctx.reporter {
        ReporterType::Default | ReporterType::AppendOnly => run_logout!(DefaultReporter),
        ReporterType::Ndjson => run_logout!(NdjsonReporter),
        ReporterType::Silent => run_logout!(SilentReporter),
    })
}

pub(in super::super) fn search<'a>(
    ctx: &RunCtx<'a>,
    args: SearchArgs,
) -> miette::Result<CommandFuture<'a>> {
    let cfg: &Config = (ctx.config)()?;
    Ok(Box::pin(async move {
        let output = args.run(cfg).await?;
        if !output.is_empty() {
            println!("{output}");
        }
        Ok(())
    }))
}
