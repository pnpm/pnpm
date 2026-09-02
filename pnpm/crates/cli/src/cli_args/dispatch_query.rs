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
    config::{ConfigArgs, ConfigGetAliasArgs, ConfigSetAliasArgs, ConfigSubcommand},
    deprecate::DeprecateArgs,
    dispatch::{CommandFuture, RunCtx, apply_update_config},
    dist_tag::DistTagArgs,
    docs::DocsArgs,
    doctor::{DoctorArgs, DoctorOutcome},
    find_hash::FindHashArgs,
    ignored_builds::IgnoredBuildsArgs,
    lane::LaneArgs,
    licenses::LicensesArgs,
    list::ListArgs,
    login::LoginArgs,
    logout::LogoutArgs,
    not_implemented::NotImplementedError,
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
    shim::ShimArgs,
    stage::StageArgs,
    star::StarArgs,
    stars::StarsArgs,
    store::StoreCommand,
    team::TeamArgs,
    undeprecate::UndeprecateArgs,
    unpublish::UnpublishArgs,
    unstar::UnstarArgs,
    version::VersionArgs,
    view::ViewArgs,
    why::WhyArgs,
    with::WithArgs,
};
use crate::{State, config_deps::prepare_config};
use clap::CommandFactory;
use miette::Context;
use pnpm_config::Config;
use pnpm_default_reporter::DefaultReporter;
use pnpm_reporter::{NdjsonReporter, SilentReporter};

pub(super) fn recursive<'a>(_ctx: &RunCtx<'a>) -> miette::Result<CommandFuture<'a>> {
    Ok(Box::pin(async move {
        let mut cmd = crate::cli_args::CliArgs::command();
        let _ = cmd.find_subcommand_mut("recursive").expect("recursive subcommand").print_help();
        #[expect(clippy::exit, reason = "`recursive` exits non-zero, mirroring pnpm")]
        std::process::exit(1);
    }))
}

// `outdated` is a read-only query: it prints a report to stdout and never
// installs. The reporter type only routes the `globalWarn` channel (skipped
// GitHub Actions repositories). It reports back whether any dependency was
// outdated; process termination stays here, at the top-level harness, rather
// than inside the command.
pub(super) fn outdated<'a>(
    ctx: &RunCtx<'a>,
    args: OutdatedArgs,
) -> miette::Result<CommandFuture<'a>> {
    if args.global {
        let config = (ctx.global_config)()?;
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
    let config = (ctx.config)()?;
    let dir = ctx.dir;
    let manifest_path = ctx.manifest_path.to_path_buf();
    let reporter = ctx.reporter;
    Ok(Box::pin(async move {
        apply_update_config(config, dir, reporter).await?;
        let command_state =
            State::init(manifest_path, config, false).wrap_err("initialize the state")?;
        let outcome = match reporter {
            ReporterType::Default | ReporterType::AppendOnly => {
                args.run::<DefaultReporter>(command_state).await?
            }
            ReporterType::Ndjson => args.run::<NdjsonReporter>(command_state).await?,
            ReporterType::Silent => args.run::<SilentReporter>(command_state).await?,
        };
        if outcome == OutdatedOutcome::Outdated {
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
    let config = (ctx.config)()?;
    let dir = ctx.dir;
    let recursive = ctx.recursive;
    Ok(Box::pin(async move { args.run(config, dir, recursive).await }))
}

pub(super) fn ll<'a>(ctx: &RunCtx<'a>, mut args: ListArgs) -> miette::Result<CommandFuture<'a>> {
    args.long = true;
    let config = (ctx.config)()?;
    let dir = ctx.dir;
    let recursive = ctx.recursive;
    Ok(Box::pin(async move { args.run(config, dir, recursive).await }))
}

pub(super) fn licenses<'a>(
    ctx: &RunCtx<'a>,
    args: LicensesArgs,
) -> miette::Result<CommandFuture<'a>> {
    let config = (ctx.config)()?;
    let dir = ctx.dir;
    let recursive = ctx.recursive;
    Ok(Box::pin(async move { args.run(config, dir, recursive).await }))
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
        if args.run(cfg, dir, recursive)? != PeersOutcome::NoIssues {
            #[expect(
                clippy::exit,
                reason = "`peers` exits non-zero when peer issues are found or the subcommand is unknown, mirroring pnpm"
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

pub(super) fn change<'a>(ctx: &RunCtx<'a>, args: ChangeArgs) -> miette::Result<CommandFuture<'a>> {
    let cfg: &Config = (ctx.config)()?;
    Ok(Box::pin(async move { args.run(cfg).await }))
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
    let dir = ctx.dir;
    let recursive = ctx.recursive;
    let reporter = ctx.reporter;
    Ok(Box::pin(async move {
        match reporter {
            ReporterType::Default | ReporterType::AppendOnly => {
                args.run::<DefaultReporter>(cfg, dir, recursive).await
            }
            ReporterType::Ndjson => args.run::<NdjsonReporter>(cfg, dir, recursive).await,
            ReporterType::Silent => args.run::<SilentReporter>(cfg, dir, recursive).await,
        }
    }))
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

pub(super) fn unpublish<'a>(
    ctx: &RunCtx<'a>,
    args: UnpublishArgs,
) -> miette::Result<CommandFuture<'a>> {
    let cfg: &Config = (ctx.config)()?;
    async fn print_output<Reporter: pnpm_reporter::Reporter>(
        args: UnpublishArgs,
        cfg: &Config,
    ) -> miette::Result<()> {
        if let Some(output) = args.run::<Reporter>(cfg).await? {
            let output = super::sanitize::sanitize(&output);
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

// `view` is a read-only registry query: it resolves the package metadata
// (and, when the package name is omitted, the nearest manifest's name from
// `ctx.dir`), then prints the requested fields, a JSON dump, or the formatted
// summary its handler returns. No lockfile or install pipeline, so it
// dispatches off `config()` like the other read-only registry commands.
pub(super) fn view<'a>(ctx: &RunCtx<'a>, args: ViewArgs) -> miette::Result<CommandFuture<'a>> {
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

// `doctor` reports on the installation and its environment, so it needs config
// resolved but no lockfile or install pipeline. It returns the rendered report
// rather than printing it, mirroring pnpm's handler → CLI print split, and the
// exit lives here because a failing check must fail the command — that is what
// lets the release pipeline gate a promotion on it.
pub(super) fn doctor<'a>(ctx: &RunCtx<'a>, args: DoctorArgs) -> miette::Result<CommandFuture<'a>> {
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
                let hooks = prepare_config::<DefaultReporter>(config, dir).await?;
                args.run::<DefaultReporter>(dir, config, recursive, hooks).await?
            }
            ReporterType::Ndjson => {
                let hooks = prepare_config::<NdjsonReporter>(config, dir).await?;
                args.run::<NdjsonReporter>(dir, config, recursive, hooks).await?
            }
            ReporterType::Silent => {
                let hooks = prepare_config::<SilentReporter>(config, dir).await?;
                args.run::<SilentReporter>(dir, config, recursive, hooks).await?
            }
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
    async fn run<Reporter: pnpm_reporter::Reporter>(
        args: PublishArgs,
        dir: &std::path::Path,
        config: &mut Config,
        recursive: bool,
    ) -> miette::Result<()> {
        let hooks = prepare_config::<Reporter>(config, dir).await?;
        args.run::<Reporter>(dir, config, recursive, hooks).await
    }
    if args.flags.json {
        return Ok(Box::pin(run::<SilentReporter>(args, dir, config, recursive)));
    }
    Ok(match ctx.reporter {
        ReporterType::Default | ReporterType::AppendOnly => {
            Box::pin(run::<DefaultReporter>(args, dir, config, recursive))
        }
        ReporterType::Ndjson => Box::pin(run::<NdjsonReporter>(args, dir, config, recursive)),
        ReporterType::Silent => Box::pin(run::<SilentReporter>(args, dir, config, recursive)),
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
    async fn print_output<Reporter: pnpm_reporter::Reporter>(
        args: StageArgs,
        dir: &std::path::Path,
        config: &mut Config,
        recursive: bool,
    ) -> miette::Result<()> {
        let hooks = if args.params.first().is_some_and(|subcommand| subcommand == "publish") {
            prepare_config::<Reporter>(config, dir).await?
        } else {
            Vec::new()
        };
        if let Some(output) = args.run::<Reporter>(dir, config, recursive, hooks).await? {
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
    args.run(ctx.dir, (ctx.config)()?)?;
    Ok(Box::pin(std::future::ready(Ok(()))))
}

pub(super) fn prefix<'a>(ctx: &RunCtx<'a>, args: PrefixArgs) -> miette::Result<CommandFuture<'a>> {
    args.run(ctx.dir, (ctx.config)()?)?;
    Ok(Box::pin(std::future::ready(Ok(()))))
}

pub(super) fn shim<'a>(ctx: &RunCtx<'a>, args: ShimArgs) -> miette::Result<CommandFuture<'a>> {
    // Writes the global bin directory and the global `config.yaml`, so it
    // reads the configuration anchored at the pnpm home — a project the
    // command happens to run in does not get to steer either.
    let config = (ctx.global_config)()?;
    Ok(Box::pin(async move {
        print!("{}", args.run(config).await?);
        Ok(())
    }))
}

pub(super) fn config<'a>(ctx: &RunCtx<'a>, args: ConfigArgs) -> miette::Result<CommandFuture<'a>> {
    args.run((ctx.config)()?, ctx.dir)?;
    Ok(Box::pin(std::future::ready(Ok(()))))
}

// `pnpm get` / `pnpm set` are the top-level spellings of the two most-used
// `pnpm config` subcommands, and run the same code so the two spellings
// cannot drift.
pub(super) fn config_get<'a>(
    ctx: &RunCtx<'a>,
    args: ConfigGetAliasArgs,
) -> miette::Result<CommandFuture<'a>> {
    config(ctx, ConfigArgs { flags: args.flags, command: ConfigSubcommand::Get(args.args) })
}

pub(super) fn config_set<'a>(
    ctx: &RunCtx<'a>,
    args: ConfigSetAliasArgs,
) -> miette::Result<CommandFuture<'a>> {
    config(ctx, ConfigArgs { flags: args.flags, command: ConfigSubcommand::Set(args.args) })
}

pub(super) fn not_implemented<'a>(command: &'static str) -> miette::Result<CommandFuture<'a>> {
    Err(NotImplementedError { command }.into())
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

pub(super) fn docs<'a>(ctx: &RunCtx<'a>, args: DocsArgs) -> miette::Result<CommandFuture<'a>> {
    let cfg = (ctx.config)()?;
    Ok(Box::pin(async move { args.run::<pnpm_network_web_auth::Host>(cfg).await }))
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
    Ok(Box::pin(async move { args.run::<pnpm_network_web_auth::Host>(cfg, dir).await }))
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
