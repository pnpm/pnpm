use super::{
    access::AccessArgs,
    audit::{AuditArgs, AuditOutcome},
    bin::BinArgs,
    bugs::BugsArgs,
    cache::CacheCommand,
    cat_file::CatFileArgs,
    cat_index::CatIndexArgs,
    change::ChangeArgs,
    clean::CleanArgs,
    config::ConfigArgs,
    deprecate::DeprecateArgs,
    dispatch::{CommandFuture, RunCtx},
    dist_tag::DistTagArgs,
    docs::DocsArgs,
    find_hash::FindHashArgs,
    ignored_builds::IgnoredBuildsArgs,
    lane::LaneArgs,
    list::ListArgs,
    login::LoginArgs,
    logout::LogoutArgs,
    outdated::{OutdatedArgs, OutdatedOutcome},
    owner::OwnerArgs,
    pack::PackArgs,
    pack_app::PackAppArgs,
    peers::{PeersArgs, PeersOutcome},
    ping::PingArgs,
    prefix::PrefixArgs,
    publish::PublishArgs,
    repo::RepoArgs,
    reporter::ReporterType,
    root::RootArgs,
    sbom::SbomArgs,
    search::SearchArgs,
    self_update::SelfUpdateArgs,
    setup::SetupArgs,
    stage::StageArgs,
    star::StarArgs,
    stars::StarsArgs,
    store::StoreCommand,
    team::TeamArgs,
    undeprecate::UndeprecateArgs,
    unstar::UnstarArgs,
    version::VersionArgs,
    why::WhyArgs,
    with::WithArgs,
};
use pacquet_config::Config;
use pacquet_default_reporter::DefaultReporter;
use pacquet_reporter::{NdjsonReporter, SilentReporter};

// `outdated` is a read-only query: it prints a report to stdout and never
// installs, so it has no reporter-typed install pipeline to dispatch on. It
// reports back whether any dependency was outdated; process termination stays
// here, at the top-level harness, rather than inside the command.
pub(super) fn outdated<'a>(
    ctx: &RunCtx<'a>,
    args: OutdatedArgs,
) -> miette::Result<CommandFuture<'a>> {
    if args.global {
        let config = (ctx.config)()?;
        return Ok(Box::pin(async move {
            if args.run_global(config).await? == OutdatedOutcome::Outdated {
                #[expect(
                    clippy::exit,
                    reason = "`outdated` exits non-zero when a dependency is outdated, mirroring pnpm"
                )]
                std::process::exit(1);
            }
            Ok(())
        }));
    }
    let command_state = (ctx.state)(false)?;
    Ok(Box::pin(async move {
        if args.run(command_state).await? == OutdatedOutcome::Outdated {
            #[expect(
                clippy::exit,
                reason = "`outdated` exits non-zero when a dependency is outdated, mirroring pnpm"
            )]
            std::process::exit(1);
        }
        Ok(())
    }))
}

pub(super) fn audit<'a>(ctx: &RunCtx<'a>, args: AuditArgs) -> miette::Result<CommandFuture<'a>> {
    let command_state = (ctx.state)(true)?;
    macro_rules! run_audit {
        ($reporter:ty) => {
            Box::pin(async move {
                if args.run::<$reporter>(command_state).await? == AuditOutcome::Vulnerable {
                    #[expect(
                        clippy::exit,
                        reason = "`audit` exits non-zero when vulnerabilities are found, mirroring pnpm"
                    )]
                    std::process::exit(1);
                }
                Ok(())
            })
        };
    }
    Ok(match ctx.reporter {
        ReporterType::Default | ReporterType::AppendOnly => run_audit!(DefaultReporter),
        ReporterType::Ndjson => run_audit!(NdjsonReporter),
        ReporterType::Silent => run_audit!(SilentReporter),
    })
}

pub(super) fn list<'a>(ctx: &RunCtx<'a>, args: ListArgs) -> miette::Result<CommandFuture<'a>> {
    args.run((ctx.config)()?, ctx.dir, ctx.recursive)?;
    Ok(Box::pin(std::future::ready(Ok(()))))
}

pub(super) fn ll<'a>(ctx: &RunCtx<'a>, mut args: ListArgs) -> miette::Result<CommandFuture<'a>> {
    args.long = true;
    args.run((ctx.config)()?, ctx.dir, ctx.recursive)?;
    Ok(Box::pin(std::future::ready(Ok(()))))
}

pub(super) fn why<'a>(ctx: &RunCtx<'a>, args: WhyArgs) -> miette::Result<CommandFuture<'a>> {
    Ok(Box::pin(args.run((ctx.state)(true)?)))
}

pub(super) fn sbom<'a>(ctx: &RunCtx<'a>, args: SbomArgs) -> miette::Result<CommandFuture<'a>> {
    Ok(Box::pin(args.run((ctx.state)(true)?)))
}

pub(super) fn peers<'a>(ctx: &RunCtx<'a>, args: PeersArgs) -> miette::Result<CommandFuture<'a>> {
    let cfg: &Config = (ctx.config)()?;
    let recursive = ctx.recursive;
    let dir = ctx.dir;
    Ok(Box::pin(async move {
        if args.run(cfg, dir, recursive)? == PeersOutcome::IssuesFound {
            #[expect(
                clippy::exit,
                reason = "`peers` exits non-zero when peer issues are found, mirroring pnpm"
            )]
            std::process::exit(1);
        }
        Ok(())
    }))
}

// `whoami` is a read-only registry query: it resolves the default registry's
// auth header from config and GETs `-/whoami`, with no lockfile or install
// pipeline. It needs an async future for the request but no reporter-typed
// fan-out, so it dispatches off `config()` like the other read-only commands.
pub(super) fn whoami<'a>(ctx: &RunCtx<'a>) -> miette::Result<CommandFuture<'a>> {
    let cfg: &Config = (ctx.config)()?;
    Ok(Box::pin(async move {
        let username = super::whoami::whoami(cfg).await?;
        println!("{}", super::sanitize::sanitize(&username));
        Ok(())
    }))
}

pub(super) fn star<'a>(ctx: &RunCtx<'a>, args: StarArgs) -> miette::Result<CommandFuture<'a>> {
    let cfg: &Config = (ctx.config)()?;
    Ok(Box::pin(async move { args.run(cfg).await }))
}

pub(super) fn unstar<'a>(ctx: &RunCtx<'a>, args: UnstarArgs) -> miette::Result<CommandFuture<'a>> {
    let cfg: &Config = (ctx.config)()?;
    Ok(Box::pin(async move { args.run(cfg).await }))
}

pub(super) fn stars<'a>(ctx: &RunCtx<'a>, args: StarsArgs) -> miette::Result<CommandFuture<'a>> {
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

pub(super) fn access<'a>(ctx: &RunCtx<'a>, args: AccessArgs) -> miette::Result<CommandFuture<'a>> {
    let cfg: &Config = (ctx.config)()?;
    Ok(Box::pin(async move {
        if let Some(output) = args.run(cfg).await? {
            let output = super::sanitize::sanitize(&output);
            if output.is_empty() {
                return Ok(());
            }
            println!("{output}");
        }
        Ok(())
    }))
}

pub(super) fn dist_tag<'a>(
    ctx: &RunCtx<'a>,
    args: DistTagArgs,
) -> miette::Result<CommandFuture<'a>> {
    let cfg: &Config = (ctx.config)()?;
    Ok(Box::pin(async move {
        if let Some(output) = args.run(cfg).await? {
            let output = super::sanitize::sanitize(&output);
            if output.is_empty() {
                return Ok(());
            }
            println!("{output}");
        }
        Ok(())
    }))
}

/// `change` and `version` are synchronous file-and-prompt commands; the
/// returned future only carries their already-computed result.
pub(super) fn change<'a>(ctx: &RunCtx<'a>, args: ChangeArgs) -> miette::Result<CommandFuture<'a>> {
    let cfg: &Config = (ctx.config)()?;
    let result = args.run(cfg);
    Ok(Box::pin(std::future::ready(result)))
}

pub(super) fn lane<'a>(ctx: &RunCtx<'a>, args: LaneArgs) -> miette::Result<CommandFuture<'a>> {
    let cfg: &Config = (ctx.config)()?;
    let result = args.run(cfg);
    Ok(Box::pin(std::future::ready(result)))
}

pub(super) fn version<'a>(
    ctx: &RunCtx<'a>,
    args: VersionArgs,
) -> miette::Result<CommandFuture<'a>> {
    let cfg: &Config = (ctx.config)()?;
    let recursive = ctx.recursive;
    Ok(Box::pin(async move { args.run(cfg, recursive).await }))
}

pub(super) fn deprecate<'a>(
    ctx: &RunCtx<'a>,
    args: DeprecateArgs,
) -> miette::Result<CommandFuture<'a>> {
    let cfg: &Config = (ctx.config)()?;
    Ok(Box::pin(async move {
        if let Some(output) = args.run(cfg).await? {
            let output = super::sanitize::sanitize(&output);
            if output.is_empty() {
                return Ok(());
            }
            println!("{output}");
        }
        Ok(())
    }))
}

pub(super) fn undeprecate<'a>(
    ctx: &RunCtx<'a>,
    args: UndeprecateArgs,
) -> miette::Result<CommandFuture<'a>> {
    let cfg: &Config = (ctx.config)()?;
    Ok(Box::pin(async move {
        if let Some(output) = args.run(cfg).await? {
            let output = super::sanitize::sanitize(&output);
            if output.is_empty() {
                return Ok(());
            }
            println!("{output}");
        }
        Ok(())
    }))
}

pub(super) fn team<'a>(ctx: &RunCtx<'a>, args: TeamArgs) -> miette::Result<CommandFuture<'a>> {
    let cfg: &Config = (ctx.config)()?;
    Ok(Box::pin(async move {
        if let Some(output) = args.run(cfg).await? {
            let output = super::sanitize::sanitize(&output);
            if output.is_empty() {
                return Ok(());
            }
            println!("{output}");
        }
        Ok(())
    }))
}

pub(super) fn owner<'a>(ctx: &RunCtx<'a>, args: OwnerArgs) -> miette::Result<CommandFuture<'a>> {
    let cfg: &Config = (ctx.config)()?;
    Ok(Box::pin(async move {
        if let Some(output) = args.run(cfg).await? {
            let output = super::sanitize::sanitize(&output);
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
pub(super) fn ping<'a>(ctx: &RunCtx<'a>, args: PingArgs) -> miette::Result<CommandFuture<'a>> {
    let cfg: &Config = (ctx.config)()?;
    Ok(Box::pin(async move {
        let report = args.run(cfg).await?;
        println!("{report}");
        Ok(())
    }))
}

// `pack` prints the tarball summary (or JSON) its handler returns; the
// reporter type only affects the lifecycle-script output, so it's threaded
// into `run` and the result printed here, mirroring pnpm's `handler` → CLI
// print split. `run` is async (it may invoke `beforePacking` pnpmfile
// hooks), so the work is deferred into the returned future.
pub(super) fn pack<'a>(ctx: &RunCtx<'a>, args: PackArgs) -> miette::Result<CommandFuture<'a>> {
    let config = (ctx.config)()?;
    let dir = ctx.dir;
    let recursive = ctx.recursive;
    let reporter = ctx.reporter;
    Ok(Box::pin(async move {
        let output = match reporter {
            ReporterType::Default | ReporterType::AppendOnly => {
                args.run::<DefaultReporter>(dir, config, recursive).await?
            }
            ReporterType::Ndjson => args.run::<NdjsonReporter>(dir, config, recursive).await?,
            ReporterType::Silent => args.run::<SilentReporter>(dir, config, recursive).await?,
        };
        if !output.is_empty() {
            println!("{output}");
        }
        Ok(())
    }))
}

/// `publish` packs the project, runs its prepublish/publish lifecycle scripts,
/// and uploads the tarball. Co-located with its sibling `pack` (both come from
/// pnpm's `releasing/commands`).
///
/// `dir` / `config` / `recursive` are read off `ctx` here, before the boxed
/// future, so the future captures only owned/concrete values and never holds
/// `&RunCtx` — whose higher-ranked config closures would otherwise make the
/// boxed [`CommandFuture`] not `Send`.
pub(super) fn publish<'a>(
    ctx: &RunCtx<'a>,
    mut args: PublishArgs,
) -> miette::Result<CommandFuture<'a>> {
    let config = (ctx.config)()?;
    let dir = ctx.dir;
    let recursive = ctx.recursive;
    args.flags.report_summary |= ctx.recursive_report_summary;
    if args.flags.json {
        return Ok(Box::pin(args.run::<SilentReporter>(dir, config, recursive)));
    }
    Ok(match ctx.reporter {
        ReporterType::Default | ReporterType::AppendOnly => {
            Box::pin(args.run::<DefaultReporter>(dir, config, recursive))
        }
        ReporterType::Ndjson => Box::pin(args.run::<NdjsonReporter>(dir, config, recursive)),
        ReporterType::Silent => Box::pin(args.run::<SilentReporter>(dir, config, recursive)),
    })
}

/// `stage` shares `publish`'s dispatch shape: the values are read off `ctx`
/// before the boxed future so it captures only owned/concrete values (see
/// [`publish`]), and the subcommand's output is printed here, sanitized,
/// mirroring pnpm's `handler` → CLI print split.
pub(super) fn stage<'a>(
    ctx: &RunCtx<'a>,
    mut args: StageArgs,
) -> miette::Result<CommandFuture<'a>> {
    let config = (ctx.config)()?;
    let dir = ctx.dir;
    let recursive = ctx.recursive;
    args.flags.report_summary |= ctx.recursive_report_summary;
    async fn print_output<Reporter: pacquet_reporter::Reporter>(
        args: StageArgs,
        dir: &std::path::Path,
        config: &Config,
        recursive: bool,
    ) -> miette::Result<()> {
        if let Some(output) = args.run::<Reporter>(dir, config, recursive).await? {
            let output = super::sanitize::sanitize(&output);
            if !output.is_empty() {
                println!("{output}");
            }
        }
        Ok(())
    }
    Ok(match ctx.reporter {
        ReporterType::Default | ReporterType::AppendOnly => {
            Box::pin(print_output::<DefaultReporter>(args, dir, config, recursive))
        }
        ReporterType::Ndjson => {
            Box::pin(print_output::<NdjsonReporter>(args, dir, config, recursive))
        }
        ReporterType::Silent => {
            Box::pin(print_output::<SilentReporter>(args, dir, config, recursive))
        }
    })
}

pub(super) fn bin<'a>(ctx: &RunCtx<'a>, args: BinArgs) -> miette::Result<CommandFuture<'a>> {
    args.run(ctx.dir, (ctx.config)()?)?;
    Ok(Box::pin(std::future::ready(Ok(()))))
}

pub(super) fn clean<'a>(
    ctx: &RunCtx<'a>,
    args: CleanArgs,
    command_name: &'a str,
) -> miette::Result<CommandFuture<'a>> {
    args.run(ctx, command_name)?;
    Ok(Box::pin(std::future::ready(Ok(()))))
}

pub(super) fn root<'a>(ctx: &RunCtx<'a>, args: RootArgs) -> miette::Result<CommandFuture<'a>> {
    args.run(ctx.dir)?;
    Ok(Box::pin(std::future::ready(Ok(()))))
}

pub(super) fn prefix<'a>(ctx: &RunCtx<'a>, args: PrefixArgs) -> miette::Result<CommandFuture<'a>> {
    args.run(ctx.dir)?;
    Ok(Box::pin(std::future::ready(Ok(()))))
}

pub(super) fn config<'a>(ctx: &RunCtx<'a>, args: ConfigArgs) -> miette::Result<CommandFuture<'a>> {
    args.run((ctx.config)()?, ctx.dir)?;
    Ok(Box::pin(std::future::ready(Ok(()))))
}

// `pack-app` reads `pnpm.app` from package.json, resolves a Node.js version
// over the network, and shells out to build the SEA executables. It needs
// config (proxy / TLS / registry) and the canonicalized `--dir` but no
// lockfile or install pipeline, so it dispatches off `config()` like the other
// read-only commands.
pub(super) fn pack_app<'a>(
    ctx: &RunCtx<'a>,
    args: PackAppArgs,
) -> miette::Result<CommandFuture<'a>> {
    let cfg: &Config = (ctx.config)()?;
    let dir = ctx.dir;
    Ok(Box::pin(async move { args.run(cfg, dir).await }))
}

pub(super) fn repo<'a>(ctx: &RunCtx<'a>, args: RepoArgs) -> miette::Result<CommandFuture<'a>> {
    let cfg = (ctx.config)()?;
    let dir = ctx.dir;
    Ok(match ctx.reporter {
        ReporterType::Default | ReporterType::AppendOnly => {
            Box::pin(async move { args.run::<DefaultReporter>(cfg, dir).await })
        }
        ReporterType::Ndjson => Box::pin(async move { args.run::<NdjsonReporter>(cfg, dir).await }),
        ReporterType::Silent => Box::pin(async move { args.run::<SilentReporter>(cfg, dir).await }),
    })
}

pub(super) fn docs<'a>(ctx: &RunCtx<'a>, args: DocsArgs) -> miette::Result<CommandFuture<'a>> {
    let cfg = (ctx.config)()?;
    Ok(Box::pin(async move { args.run(cfg).await }))
}

pub(super) fn with<'a>(ctx: &RunCtx<'a>, args: WithArgs) -> miette::Result<CommandFuture<'a>> {
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

pub(super) fn self_update<'a>(
    ctx: &RunCtx<'a>,
    args: SelfUpdateArgs,
) -> miette::Result<CommandFuture<'a>> {
    // Refuse corepack before loading project config, so a broken `.npmrc`
    // / workspace config can't mask the corepack refusal.
    super::self_update::reject_if_corepack()?;
    let config = (ctx.config)()?;
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
pub(super) fn setup<'a>(ctx: &RunCtx<'a>, args: SetupArgs) -> miette::Result<CommandFuture<'a>> {
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

// `login` (a.k.a. `adduser`) authenticates with the registry and writes the
// token to `auth.ini`. Like `logout` it needs config (registry, config dir,
// network settings) but no lockfile or install pipeline. Its `globalInfo`
// messages (the auth URL / QR code, the "Logged in as ..." line) route through
// the reporter, so the reporter type is threaded through `run`.
pub(super) fn login<'a>(ctx: &RunCtx<'a>, args: LoginArgs) -> miette::Result<CommandFuture<'a>> {
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
pub(super) fn logout<'a>(ctx: &RunCtx<'a>, args: LogoutArgs) -> miette::Result<CommandFuture<'a>> {
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

pub(super) fn store<'a>(
    ctx: &RunCtx<'a>,
    command: StoreCommand,
) -> miette::Result<CommandFuture<'a>> {
    command.run(|| (ctx.config)().map(|m| &*m))?;
    Ok(Box::pin(std::future::ready(Ok(()))))
}

pub(super) fn cache<'a>(
    ctx: &RunCtx<'a>,
    command: CacheCommand,
) -> miette::Result<CommandFuture<'a>> {
    command.run((ctx.config)()?)?;
    Ok(Box::pin(std::future::ready(Ok(()))))
}

pub(super) fn cat_file<'a>(
    ctx: &RunCtx<'a>,
    args: CatFileArgs,
) -> miette::Result<CommandFuture<'a>> {
    args.run(|| (ctx.config)().map(|m| &*m))?;
    Ok(Box::pin(std::future::ready(Ok(()))))
}

pub(super) fn cat_index<'a>(
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

pub(super) fn ignored_builds<'a>(
    ctx: &RunCtx<'a>,
    _args: IgnoredBuildsArgs,
) -> miette::Result<CommandFuture<'a>> {
    let output = super::ignored_builds::render_ignored_builds((ctx.config)()?)?;
    print!("{output}");
    Ok(Box::pin(std::future::ready(Ok(()))))
}

pub(super) fn bugs<'a>(ctx: &RunCtx<'a>, args: BugsArgs) -> miette::Result<CommandFuture<'a>> {
    let cfg: &Config = (ctx.config)()?;
    let dir = ctx.dir;
    Ok(Box::pin(async move { args.run::<pacquet_network_web_auth::Host>(cfg, dir).await }))
}

pub(super) fn find_hash<'a>(
    ctx: &RunCtx<'a>,
    args: FindHashArgs,
) -> miette::Result<CommandFuture<'a>> {
    args.run(|| (ctx.config)().map(|m| &*m))?;
    Ok(Box::pin(std::future::ready(Ok(()))))
}

pub(super) fn search<'a>(ctx: &RunCtx<'a>, args: SearchArgs) -> miette::Result<CommandFuture<'a>> {
    let cfg: &Config = (ctx.config)()?;
    Ok(Box::pin(async move {
        let output = args.run(cfg).await?;
        if !output.is_empty() {
            println!("{output}");
        }
        Ok(())
    }))
}
